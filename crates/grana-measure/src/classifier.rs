use crate::column::{Column, Unit};
use crate::metric::Measurement;
use crate::table::{Objects, Table};

pub const SCORE_KEY: &str = "classifier_score";
pub const SCORE_LABEL: &str = "classifier";
const REGULARIZATION: f64 = 0.05;
const ITERATIONS: usize = 32;
const MINIMUM_SAMPLES: usize = 2;
const SCALE_FLOOR: f64 = 1.0e-6;

#[derive(Clone, Debug, PartialEq)]
pub struct Classifier {
    features: Vec<Measurement>,
    mean: Vec<f64>,
    scale: Vec<f64>,
    weights: Vec<f64>,
    bias: f64,
    positives: usize,
    negatives: usize,
    accuracy: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassifierState {
    pub features: Vec<Measurement>,
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub weights: Vec<f64>,
    pub bias: f64,
    pub positives: usize,
    pub negatives: usize,
    pub accuracy: f32,
}

impl Classifier {
    pub fn train(
        table: &Table,
        keys: &[String],
        features: &[Measurement],
        positives: &[usize],
    ) -> Result<Self, String> {
        if features.is_empty() {
            return Err("a classifier trains on at least one measurement".to_string());
        }
        let mut columns = Vec::with_capacity(features.len());
        for feature in features {
            let key = feature.key(keys);
            let column = table
                .column(&key)
                .ok_or_else(|| format!("the table holds no {key} measurement"))?;
            columns.push(column);
        }
        let rows = table.rows();
        if rows == 0 {
            return Err("the object set holds no measurement to train on".to_string());
        }
        let mut positive = vec![false; rows];
        for row in positives {
            if *row >= rows {
                return Err("a training row lies outside the measurement table".to_string());
            }
            positive[*row] = true;
        }
        let mut samples = Vec::with_capacity(rows);
        let mut labels = Vec::with_capacity(rows);
        for (row, kept) in positive.iter().enumerate() {
            let mut sample = Vec::with_capacity(columns.len());
            let mut finite = true;
            for column in &columns {
                let value = table.value(row, *column);
                if !value.is_finite() {
                    finite = false;
                    break;
                }
                sample.push(f64::from(value));
            }
            if finite {
                samples.push(sample);
                labels.push(*kept);
            }
        }
        let positives = labels.iter().filter(|label| **label).count();
        let negatives = labels.len() - positives;
        if positives < MINIMUM_SAMPLES || negatives < MINIMUM_SAMPLES {
            return Err(format!(
                "a classifier needs at least {MINIMUM_SAMPLES} kept and {MINIMUM_SAMPLES} excluded objects with finite measurements, and the gate keeps {positives} of {}",
                labels.len()
            ));
        }
        let count = samples.len();
        let width = features.len();
        let mut mean = vec![0.0f64; width];
        for sample in &samples {
            for (slot, value) in sample.iter().enumerate() {
                mean[slot] += value;
            }
        }
        for value in &mut mean {
            *value /= count as f64;
        }
        let mut scale = vec![0.0f64; width];
        for sample in &samples {
            for (slot, value) in sample.iter().enumerate() {
                scale[slot] += (value - mean[slot]).powi(2);
            }
        }
        for value in &mut scale {
            *value = (*value / count as f64).sqrt().max(SCALE_FLOOR);
        }
        let design = samples
            .iter()
            .map(|sample| {
                let mut row = Vec::with_capacity(width + 1);
                row.push(1.0f64);
                for (slot, value) in sample.iter().enumerate() {
                    row.push((value - mean[slot]) / scale[slot]);
                }
                row
            })
            .collect::<Vec<_>>();
        let weights = class_weights(&labels, positives, negatives);
        let mut coefficients = vec![0.0f64; width + 1];
        for _ in 0..ITERATIONS {
            let (gradient, hessian) = derivatives(&design, &labels, &weights, &coefficients);
            let Some(step) = solve(&hessian, &gradient) else {
                break;
            };
            let mut converged = true;
            for (coefficient, delta) in coefficients.iter_mut().zip(&step) {
                *coefficient -= delta;
                converged &= delta.abs() < 1.0e-8;
            }
            if converged {
                break;
            }
        }
        let mut classifier = Self {
            features: features.to_vec(),
            mean,
            scale,
            weights: coefficients[1..].to_vec(),
            bias: coefficients[0],
            positives,
            negatives,
            accuracy: 0.0,
        };
        classifier.accuracy = classifier.balanced_accuracy(&design, &labels);
        Ok(classifier)
    }

    pub fn features(&self) -> &[Measurement] {
        &self.features
    }

    pub fn positives(&self) -> usize {
        self.positives
    }

    pub fn negatives(&self) -> usize {
        self.negatives
    }

    pub fn accuracy(&self) -> f32 {
        self.accuracy
    }

    pub fn column(&self) -> Column {
        Column::new(SCORE_LABEL, SCORE_KEY, Unit::Count, 3)
    }

    pub fn aggregate_column(&self, aggregate: crate::table::Aggregate) -> Column {
        Column::new(
            format!("{SCORE_LABEL} {}", aggregate.name()),
            SCORE_KEY,
            Unit::Count,
            3,
        )
    }

    pub fn score(&self, objects: Objects<'_>, object: usize) -> f32 {
        let mut logit = self.bias;
        for (slot, feature) in self.features.iter().enumerate() {
            let value = f64::from(feature.value(objects, object));
            if !value.is_finite() {
                return f32::NAN;
            }
            logit += self.weights[slot] * (value - self.mean[slot]) / self.scale[slot];
        }
        sigmoid(logit) as f32
    }

    pub fn scores(&self, table: &Table, keys: &[String]) -> Vec<f32> {
        let columns = self
            .features
            .iter()
            .map(|feature| table.column(&feature.key(keys)))
            .collect::<Option<Vec<_>>>();
        let Some(columns) = columns else {
            return vec![f32::NAN; table.rows()];
        };
        (0..table.rows())
            .map(|row| {
                let mut logit = self.bias;
                for (slot, column) in columns.iter().enumerate() {
                    let value = f64::from(table.value(row, *column));
                    if !value.is_finite() {
                        return f32::NAN;
                    }
                    logit += self.weights[slot] * (value - self.mean[slot]) / self.scale[slot];
                }
                sigmoid(logit) as f32
            })
            .collect()
    }

    pub fn state(&self) -> ClassifierState {
        ClassifierState {
            features: self.features.clone(),
            mean: self.mean.clone(),
            scale: self.scale.clone(),
            weights: self.weights.clone(),
            bias: self.bias,
            positives: self.positives,
            negatives: self.negatives,
            accuracy: self.accuracy,
        }
    }

    pub fn restore(state: ClassifierState) -> Result<Self, String> {
        let ClassifierState {
            features,
            mean,
            scale,
            weights,
            bias,
            positives,
            negatives,
            accuracy,
        } = state;
        if features.is_empty() {
            return Err("a classifier keeps at least one measurement".to_string());
        }
        if mean.len() != features.len()
            || scale.len() != features.len()
            || weights.len() != features.len()
        {
            return Err("a classifier keeps one weight per measurement".to_string());
        }
        if !mean
            .iter()
            .chain(&scale)
            .chain(&weights)
            .chain([&bias])
            .all(|value| value.is_finite())
        {
            return Err("a classifier keeps finite coefficients".to_string());
        }
        if scale.iter().any(|value| *value <= 0.0) {
            return Err("a classifier scales its measurements by positive factors".to_string());
        }
        if accuracy.is_finite() {
            Ok(Self {
                features,
                mean,
                scale,
                weights,
                bias,
                positives,
                negatives,
                accuracy,
            })
        } else {
            Err("a classifier records a finite accuracy".to_string())
        }
    }

    fn balanced_accuracy(&self, design: &[Vec<f64>], labels: &[bool]) -> f32 {
        let mut kept = (0usize, 0usize);
        let mut excluded = (0usize, 0usize);
        for (row, label) in design.iter().zip(labels) {
            let logit = self.bias
                + row[1..]
                    .iter()
                    .zip(&self.weights)
                    .map(|(value, weight)| value * weight)
                    .sum::<f64>();
            let predicted = logit >= 0.0;
            if *label {
                kept.1 += 1;
                kept.0 += usize::from(predicted);
            } else {
                excluded.1 += 1;
                excluded.0 += usize::from(!predicted);
            }
        }
        let sensitivity = ratio(kept);
        let specificity = ratio(excluded);
        0.5 * (sensitivity + specificity)
    }
}

fn ratio(counts: (usize, usize)) -> f32 {
    if counts.1 == 0 {
        0.0
    } else {
        counts.0 as f32 / counts.1 as f32
    }
}

fn class_weights(labels: &[bool], positives: usize, negatives: usize) -> Vec<f64> {
    let total = labels.len() as f64;
    let kept = total / (2.0 * positives as f64);
    let excluded = total / (2.0 * negatives as f64);
    labels
        .iter()
        .map(|label| if *label { kept } else { excluded })
        .collect()
}

fn derivatives(
    design: &[Vec<f64>],
    labels: &[bool],
    weights: &[f64],
    coefficients: &[f64],
) -> (Vec<f64>, Vec<Vec<f64>>) {
    let width = coefficients.len();
    let count = design.len() as f64;
    let mut gradient = vec![0.0f64; width];
    let mut hessian = vec![vec![0.0f64; width]; width];
    for ((row, label), weight) in design.iter().zip(labels).zip(weights) {
        let logit = row
            .iter()
            .zip(coefficients)
            .map(|(value, coefficient)| value * coefficient)
            .sum::<f64>();
        let probability = sigmoid(logit);
        let target = if *label { 1.0 } else { 0.0 };
        let residual = weight * (probability - target) / count;
        let curvature = weight * probability * (1.0 - probability) / count;
        for (slot, value) in row.iter().enumerate() {
            gradient[slot] += residual * value;
            for (other, pair) in row.iter().enumerate() {
                hessian[slot][other] += curvature * value * pair;
            }
        }
    }
    for (slot, value) in gradient.iter_mut().enumerate().skip(1) {
        *value += REGULARIZATION * coefficients[slot];
    }
    for (slot, row) in hessian.iter_mut().enumerate().skip(1) {
        row[slot] += REGULARIZATION;
    }
    (gradient, hessian)
}

fn solve(matrix: &[Vec<f64>], vector: &[f64]) -> Option<Vec<f64>> {
    let size = vector.len();
    let mut augmented = matrix
        .iter()
        .enumerate()
        .map(|(row, values)| {
            let mut line = values.clone();
            line.push(vector[row]);
            line
        })
        .collect::<Vec<_>>();
    for pivot in 0..size {
        let mut best = pivot;
        for (row, values) in augmented.iter().enumerate().skip(pivot + 1) {
            if values[pivot].abs() > augmented[best][pivot].abs() {
                best = row;
            }
        }
        if augmented[best][pivot].abs() < 1.0e-12 {
            return None;
        }
        augmented.swap(pivot, best);
        let divisor = augmented[pivot][pivot];
        for value in &mut augmented[pivot][pivot..=size] {
            *value /= divisor;
        }
        let pivot_row = augmented[pivot].clone();
        for (row, values) in augmented.iter_mut().enumerate() {
            if row == pivot {
                continue;
            }
            let factor = values[pivot];
            if factor == 0.0 {
                continue;
            }
            for (slot, value) in values[pivot..=size].iter_mut().enumerate() {
                *value -= factor * pivot_row[pivot + slot];
            }
        }
    }
    Some(augmented.iter().map(|row| row[size]).collect())
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exponential = value.exp();
        exponential / (1.0 + exponential)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design() -> (Vec<Vec<f64>>, Vec<bool>) {
        let mut samples = Vec::new();
        let mut labels = Vec::new();
        for index in 0..20 {
            let high = index % 2 == 0;
            let first = if high { 2.0 } else { 0.0 } + index as f64 * 0.01;
            let second = if high { 1.8 } else { 0.2 } + (index % 5) as f64 * 0.02;
            samples.push(vec![first, second]);
            labels.push(high);
        }
        (samples, labels)
    }

    #[test]
    fn fits_a_separable_boundary() {
        let (samples, labels) = design();
        let mut mean = vec![0.0f64; 2];
        for sample in &samples {
            for (slot, value) in sample.iter().enumerate() {
                mean[slot] += value;
            }
        }
        for value in &mut mean {
            *value /= samples.len() as f64;
        }
        let mut scale = vec![0.0f64; 2];
        for sample in &samples {
            for (slot, value) in sample.iter().enumerate() {
                scale[slot] += (value - mean[slot]).powi(2);
            }
        }
        for value in &mut scale {
            *value = (*value / samples.len() as f64).sqrt().max(SCALE_FLOOR);
        }
        let design = samples
            .iter()
            .map(|sample| {
                let mut row = vec![1.0f64];
                for (slot, value) in sample.iter().enumerate() {
                    row.push((value - mean[slot]) / scale[slot]);
                }
                row
            })
            .collect::<Vec<_>>();
        let positives = labels.iter().filter(|label| **label).count();
        let weights = class_weights(&labels, positives, labels.len() - positives);
        let mut coefficients = vec![0.0f64; 3];
        for _ in 0..ITERATIONS {
            let (gradient, hessian) = derivatives(&design, &labels, &weights, &coefficients);
            let Some(step) = solve(&hessian, &gradient) else {
                break;
            };
            for (coefficient, delta) in coefficients.iter_mut().zip(&step) {
                *coefficient -= delta;
            }
        }
        let classifier = Classifier {
            features: vec![Measurement::Volume, Measurement::Area],
            mean,
            scale,
            weights: coefficients[1..].to_vec(),
            bias: coefficients[0],
            positives,
            negatives: labels.len() - positives,
            accuracy: 0.0,
        };
        let accuracy = classifier.balanced_accuracy(&design, &labels);
        assert!(
            accuracy > 0.95,
            "the classifier separates the two groups, got {accuracy}"
        );
    }

    #[test]
    fn rejects_a_state_with_mismatched_coefficients() {
        let state = ClassifierState {
            features: vec![Measurement::Volume, Measurement::Area],
            mean: vec![0.0],
            scale: vec![1.0],
            weights: vec![1.0],
            bias: 0.0,
            positives: 2,
            negatives: 2,
            accuracy: 0.5,
        };
        assert!(Classifier::restore(state).is_err());
    }
}
