use grana_volume::Intensity;

const QUADRATIC_TERMS: usize = 10;
const HISTOGRAM_BINS: usize = 256;
const SAMPLE_FRACTION: f32 = 0.2;
const ROBUST_ITERATIONS: usize = 3;
const RIDGE: f64 = 1.0e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundSample {
    position: [f32; 3],
    level: f32,
}

impl BackgroundSample {
    pub fn new(position: [f32; 3], level: f32) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a background sample names a finite voxel"
        );
        assert!(level.is_finite(), "a background level is finite");
        Self { position, level }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Background {
    extent: [f32; 3],
    coefficients: [f64; QUADRATIC_TERMS],
    floor: f32,
    ceiling: f32,
}

impl Background {
    pub fn samples(
        count: usize,
        dims: [u32; 3],
        origin: [u32; 3],
        blocks: u32,
        intensity: Intensity,
        value: impl Fn(usize) -> f32,
    ) -> Vec<BackgroundSample> {
        assert_eq!(
            count,
            dims.iter().map(|axis| *axis as usize).product::<usize>(),
            "the values cover the sampled block"
        );
        let span = intensity.span();
        let mut samples = Vec::with_capacity((blocks * blocks * blocks) as usize);
        if span <= 0.0 {
            return samples;
        }
        let counts = [0, 1, 2].map(|axis| blocks.min(dims[axis]).max(1));
        for depth in 0..counts[2] {
            for line in 0..counts[1] {
                for column in 0..counts[0] {
                    let ranges = [0, 1, 2].map(|axis| {
                        let length = dims[axis];
                        let count = counts[axis];
                        let start = length * [column, line, depth][axis] / count;
                        let end = length * ([column, line, depth][axis] + 1) / count;
                        start..end
                    });
                    let mut bins = [0u32; HISTOGRAM_BINS];
                    let mut total = 0u32;
                    for z in ranges[2].clone() {
                        for y in ranges[1].clone() {
                            let base =
                                dims[0] as usize * (y as usize + dims[1] as usize * z as usize);
                            for x in ranges[0].clone() {
                                let unit = ((value(base + x as usize) - intensity.min()) / span)
                                    .clamp(0.0, 1.0);
                                bins[(unit * (HISTOGRAM_BINS - 1) as f32) as usize] += 1;
                                total += 1;
                            }
                        }
                    }
                    let mut target = (f64::from(total) * f64::from(SAMPLE_FRACTION)) as u32;
                    let mut bin = 0;
                    for (index, bucket) in bins.iter().enumerate() {
                        bin = index;
                        if target < *bucket {
                            break;
                        }
                        target -= *bucket;
                    }
                    let position = [0, 1, 2].map(|axis| {
                        let range = &ranges[axis];
                        origin[axis] as f32 + 0.5 * (range.start + range.end) as f32
                    });
                    let level = intensity.min() + (bin as f32 + 0.5) / HISTOGRAM_BINS as f32 * span;
                    samples.push(BackgroundSample::new(position, level));
                }
            }
        }
        samples
    }

    pub fn fit(samples: &[BackgroundSample], extent: [f32; 3], intensity: Intensity) -> Self {
        assert!(!samples.is_empty(), "a background fit needs a sample");
        assert!(
            extent.iter().all(|value| *value > 0.0),
            "a background spans a positive extent"
        );
        let terms = terms_for(samples.len());
        let floor = intensity.min();
        let ceiling = intensity.max();
        let positions = samples
            .iter()
            .map(|sample| normalize(sample.position, extent))
            .collect::<Vec<_>>();
        let levels = samples
            .iter()
            .map(|sample| f64::from(sample.level))
            .collect::<Vec<_>>();
        let mut coefficients = constant_fit(&levels);
        let mut weights = robust_weights(&coefficients, &positions, &levels);
        for _ in 0..ROBUST_ITERATIONS {
            coefficients = fit_once(&positions, &levels, &weights, terms).unwrap_or(coefficients);
            let next = robust_weights(&coefficients, &positions, &levels);
            if next == weights {
                break;
            }
            weights = next;
        }
        Self {
            extent,
            coefficients,
            floor,
            ceiling,
        }
    }

    pub fn value(&self, voxel: [f32; 3]) -> f32 {
        let basis = normalize(voxel, self.extent);
        self.clamp(evaluate(&self.coefficients, basis))
    }

    pub fn row(&self, voxel: [f32; 3], count: usize, out: &mut [f32]) {
        assert!(
            out.len() >= count,
            "the row buffer holds every requested voxel"
        );
        let step = 2.0 / f64::from(self.extent[0]);
        let start = normalize(voxel, self.extent);
        let coefficients = &self.coefficients;
        let c0 = coefficients[0]
            + coefficients[2] * start[1]
            + coefficients[5] * start[1] * start[1]
            + coefficients[3] * start[2]
            + coefficients[6] * start[2] * start[2]
            + coefficients[9] * start[1] * start[2];
        let c1 = coefficients[1] + coefficients[7] * start[1] + coefficients[8] * start[2];
        let c2 = coefficients[4];
        let mut value = c2 * start[0] * start[0] + c1 * start[0] + c0;
        let mut slope = 2.0 * c2 * start[0] + c1;
        let second = c2 * step * step;
        for slot in out.iter_mut().take(count) {
            *slot = self.clamp(value);
            value += slope * step + second;
            slope += 2.0 * second;
        }
    }

    fn clamp(&self, value: f64) -> f32 {
        (value as f32).clamp(self.floor, self.ceiling)
    }
}

fn terms_for(samples: usize) -> usize {
    if samples >= 20 {
        QUADRATIC_TERMS
    } else if samples >= 6 {
        4
    } else {
        1
    }
}

fn normalize(voxel: [f32; 3], extent: [f32; 3]) -> [f64; 3] {
    [0, 1, 2].map(|axis| {
        let length = f64::from(extent[axis]);
        (2.0 * (f64::from(voxel[axis]) + 0.5) / length - 1.0).clamp(-1.0, 1.0)
    })
}

fn basis(terms: usize, point: [f64; 3]) -> [f64; QUADRATIC_TERMS] {
    let [u, v, w] = point;
    let mut values = [0.0; QUADRATIC_TERMS];
    values[0] = 1.0;
    if terms > 1 {
        values[1] = u;
        values[2] = v;
        values[3] = w;
    }
    if terms > 4 {
        values[4] = u * u;
        values[5] = v * v;
        values[6] = w * w;
        values[7] = u * v;
        values[8] = u * w;
        values[9] = v * w;
    }
    values
}

fn evaluate(coefficients: &[f64; QUADRATIC_TERMS], point: [f64; 3]) -> f64 {
    coefficients
        .iter()
        .zip(basis(QUADRATIC_TERMS, point))
        .map(|(coefficient, value)| coefficient * value)
        .sum()
}

fn constant_fit(levels: &[f64]) -> [f64; QUADRATIC_TERMS] {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut coefficients = [0.0; QUADRATIC_TERMS];
    coefficients[0] = sorted[sorted.len() / 2];
    coefficients
}

fn robust_weights(
    coefficients: &[f64; QUADRATIC_TERMS],
    positions: &[[f64; 3]],
    levels: &[f64],
) -> Vec<f64> {
    let residuals = positions
        .iter()
        .zip(levels)
        .map(|(basis, level)| (level - evaluate(coefficients, *basis)).abs())
        .collect::<Vec<_>>();
    let scale = robust_scale(&residuals);
    residuals
        .iter()
        .map(|residual| (1.0 / (1.0 + (residual / scale).powi(2))).max(0.05))
        .collect()
}

fn fit_once(
    positions: &[[f64; 3]],
    levels: &[f64],
    weights: &[f64],
    terms: usize,
) -> Option<[f64; QUADRATIC_TERMS]> {
    let mut matrix = [[0.0f64; QUADRATIC_TERMS]; QUADRATIC_TERMS];
    let mut rhs = [0.0f64; QUADRATIC_TERMS];
    for ((point, level), weight) in positions.iter().zip(levels).zip(weights) {
        let row = basis(terms, *point);
        for (left, value) in row.iter().take(terms).enumerate() {
            let weighted = value * weight;
            rhs[left] += weighted * level;
            for (slot, right) in matrix[left].iter_mut().take(terms).zip(&row) {
                *slot += weighted * right;
            }
        }
    }
    let mut trace = 0.0;
    for (index, row) in matrix.iter().enumerate().take(terms) {
        trace += row[index];
    }
    let ridge = RIDGE * trace.max(1.0);
    for (index, row) in matrix.iter_mut().enumerate().take(terms) {
        row[index] += ridge;
    }
    solve(&mut matrix, &mut rhs, terms)
}

fn solve(
    matrix: &mut [[f64; QUADRATIC_TERMS]; QUADRATIC_TERMS],
    rhs: &mut [f64; QUADRATIC_TERMS],
    terms: usize,
) -> Option<[f64; QUADRATIC_TERMS]> {
    for column in 0..terms {
        let pivot = (column..terms)
            .max_by(|left, right| {
                matrix[*left][column]
                    .abs()
                    .total_cmp(&matrix[*right][column].abs())
            })
            .expect("the pivot search covers a column");
        if matrix[pivot][column].abs() < 1.0e-12 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..terms] {
            *value /= scale;
        }
        rhs[column] /= scale;
        for row in 0..terms {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            let source = matrix[column];
            for (target, value) in matrix[row][column..terms]
                .iter_mut()
                .zip(&source[column..terms])
            {
                *target -= factor * value;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut coefficients = [0.0; QUADRATIC_TERMS];
    coefficients[..terms].copy_from_slice(&rhs[..terms]);
    Some(coefficients)
}

fn robust_scale(residuals: &[f64]) -> f64 {
    let mut sorted = residuals.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let magnitude = 1.4826 * median;
    magnitude.max(1.0e-3)
}
