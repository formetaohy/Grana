use std::collections::BTreeMap;

use crate::mask::Mask;
use crate::sample::Sample;
use crate::table::Table;

pub const DENSITY_POINTS: usize = 64;

const DENSITY_SAMPLES: usize = 4096;
const OUTLIER_LIMIT: usize = 96;
const WHISKER: f64 = 1.5;

#[derive(Clone, Debug, PartialEq)]
pub struct DistributionSeries {
    count: u64,
    minimum: f64,
    maximum: f64,
    lower: f64,
    median: f64,
    upper: f64,
    mean: f64,
    deviation: f64,
    whisker_low: f64,
    whisker_high: f64,
    outliers: Vec<f32>,
    density: Vec<f32>,
}

impl DistributionSeries {
    pub fn of(values: Vec<f64>, range: (f32, f32)) -> Option<Self> {
        assert!(
            range.0.is_finite() && range.1.is_finite() && range.0 <= range.1,
            "a distribution series spans a finite range"
        );
        assert!(
            values.iter().all(|value| !value.is_finite()
                || (*value as f32) >= range.0 && (*value as f32) <= range.1),
            "a distribution series holds values inside its density range"
        );
        let mut count = 0u64;
        series_with(values, range.0, range.1, &mut count)
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn minimum(&self) -> f64 {
        self.minimum
    }

    pub fn maximum(&self) -> f64 {
        self.maximum
    }

    pub fn lower_quartile(&self) -> f64 {
        self.lower
    }

    pub fn median(&self) -> f64 {
        self.median
    }

    pub fn upper_quartile(&self) -> f64 {
        self.upper
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    pub fn deviation(&self) -> f64 {
        self.deviation
    }

    pub fn whisker_low(&self) -> f64 {
        self.whisker_low
    }

    pub fn whisker_high(&self) -> f64 {
        self.whisker_high
    }

    pub fn outliers(&self) -> &[f32] {
        &self.outliers
    }

    pub fn density(&self) -> &[f32] {
        &self.density
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistributionGroup {
    time: Option<u32>,
    label: String,
    total: DistributionSeries,
    gated: Option<DistributionSeries>,
}

impl DistributionGroup {
    pub fn time(&self) -> Option<u32> {
        self.time
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn total(&self) -> &DistributionSeries {
        &self.total
    }

    pub fn gated(&self) -> Option<&DistributionSeries> {
        self.gated.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Distribution {
    column: usize,
    groups: Vec<DistributionGroup>,
    low: f32,
    high: f32,
    total: u64,
    gated: u64,
}

impl Distribution {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn groups(&self) -> &[DistributionGroup] {
        &self.groups
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn gated(&self) -> u64 {
        self.gated
    }
}

impl Table {
    pub(crate) fn distribution(&self, column: usize, gated: &Mask) -> Distribution {
        let (low, high) = super::extent(self, column);
        let timed = (0..self.rows()).all(|row| self.observation(row).time().is_some());
        let distinct = timed && {
            let mut times = (0..self.rows())
                .filter_map(|row| self.observation(row).time())
                .collect::<Vec<_>>();
            times.sort_unstable();
            times.dedup();
            times.len() == self.rows()
        };
        let split = timed && !distinct;
        let label = distribution_label(self);
        let values = self.materialize(column);
        let mut totals = BTreeMap::<u32, Vec<f64>>::new();
        for row in 0..self.rows() {
            let value = values[row];
            if value.is_finite() {
                totals
                    .entry(distribution_key(self, row, split))
                    .or_default()
                    .push(f64::from(value));
            }
        }
        let gating = gated.count() as usize != self.rows();
        let mut gated_values = BTreeMap::<u32, Vec<f64>>::new();
        if gating {
            for row in gated.iter() {
                let value = values[row];
                if value.is_finite() {
                    gated_values
                        .entry(distribution_key(self, row, split))
                        .or_default()
                        .push(f64::from(value));
                }
            }
        }
        let mut total = 0u64;
        let mut gated_count = 0u64;
        let mut groups = Vec::with_capacity(totals.len());
        for (key, values) in totals {
            total += values.len() as u64;
            let series = series(values, low, high);
            let gated = gated_values
                .remove(&key)
                .and_then(|values| series_with(values, low, high, &mut gated_count));
            groups.push(DistributionGroup {
                time: split.then_some(key),
                label: if split {
                    format!("Frame {}", key + 1)
                } else {
                    label.clone()
                },
                total: series,
                gated,
            });
        }
        Distribution {
            column,
            groups,
            low,
            high,
            total,
            gated: if gating { gated_count } else { total },
        }
    }
}

fn distribution_key(table: &Table, row: usize, split: bool) -> u32 {
    if split {
        table
            .observation(row)
            .time()
            .expect("a split distribution groups observations by frame")
    } else {
        0
    }
}

fn distribution_label(table: &Table) -> String {
    if (0..table.rows()).any(|row| table.observation(row).object().is_some()) {
        "All objects".to_string()
    } else if (0..table.rows()).any(|row| table.observation(row).track().is_some()) {
        "All tracks".to_string()
    } else {
        "All frames".to_string()
    }
}

fn series(values: Vec<f64>, low: f32, high: f32) -> DistributionSeries {
    let mut count = 0u64;
    series_with(values, low, high, &mut count).expect("a distribution bucket holds a finite value")
}

fn series_with(
    values: Vec<f64>,
    low: f32,
    high: f32,
    count: &mut u64,
) -> Option<DistributionSeries> {
    let sample = Sample::new(values)?;
    *count += sample.len() as u64;
    let lower = sample.quantile(25.0);
    let upper = sample.quantile(75.0);
    let span = upper - lower;
    let low_fence = lower - WHISKER * span;
    let high_fence = upper + WHISKER * span;
    let whisker_low = sample
        .values()
        .iter()
        .copied()
        .find(|value| *value >= low_fence)
        .unwrap_or_else(|| sample.minimum());
    let whisker_high = sample
        .values()
        .iter()
        .rev()
        .copied()
        .find(|value| *value <= high_fence)
        .unwrap_or_else(|| sample.maximum());
    let mut outliers = sample
        .values()
        .iter()
        .copied()
        .filter(|value| *value < low_fence || *value > high_fence)
        .collect::<Vec<_>>();
    if outliers.len() > OUTLIER_LIMIT {
        let step = outliers.len().div_ceil(OUTLIER_LIMIT);
        outliers = outliers.into_iter().step_by(step).collect();
    }
    Some(DistributionSeries {
        count: sample.len() as u64,
        minimum: sample.minimum(),
        maximum: sample.maximum(),
        lower,
        median: sample.median(),
        upper,
        mean: sample.mean(),
        deviation: sample.deviation().unwrap_or(0.0),
        whisker_low,
        whisker_high,
        outliers: outliers.into_iter().map(|value| value as f32).collect(),
        density: density(&sample, low, high),
    })
}

fn density(sample: &Sample, low: f32, high: f32) -> Vec<f32> {
    let mut points = vec![0.0f32; DENSITY_POINTS];
    let span = f64::from(high) - f64::from(low);
    if sample.is_empty() || span <= 0.0 {
        return points;
    }
    let step = sample.len().div_ceil(DENSITY_SAMPLES).max(1);
    let values = sample
        .values()
        .iter()
        .step_by(step)
        .copied()
        .collect::<Vec<_>>();
    let count = values.len() as f64;
    let deviation = sample.deviation().unwrap_or(0.0);
    let interquartile = sample.quantile(75.0) - sample.quantile(25.0);
    let spread = if interquartile > 0.0 {
        deviation.min(interquartile / 1.349)
    } else {
        deviation
    };
    let floor = span / DENSITY_POINTS as f64 * 0.75;
    let bandwidth = (0.9 * spread * count.powf(-0.2)).max(floor);
    let norm = 1.0 / (count * bandwidth * std::f64::consts::TAU.sqrt());
    let mut peak = 0.0f64;
    for (index, point) in points.iter_mut().enumerate() {
        let x = f64::from(low) + (index as f64 + 0.5) / DENSITY_POINTS as f64 * span;
        let sum = values
            .iter()
            .map(|value| (-0.5 * ((x - value) / bandwidth).powi(2)).exp())
            .sum::<f64>();
        let value = sum * norm;
        *point = value as f32;
        peak = peak.max(value);
    }
    if peak > 0.0 {
        for point in &mut points {
            *point = (*point as f64 / peak) as f32;
        }
    }
    points
}
