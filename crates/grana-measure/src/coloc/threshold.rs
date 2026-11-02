use super::{ColocMoments, ColocThresholdRange, ColocThresholds};

const ZERO: f64 = 1.0e-4;
const MINIMUM: f64 = 3.0;

#[derive(Clone, Copy, Default)]
struct Sums {
    count: f64,
    a: f64,
    b: f64,
    aa: f64,
    bb: f64,
    ab: f64,
}

impl Sums {
    fn minus(self, other: Self) -> Self {
        Self {
            count: self.count - other.count,
            a: self.a - other.a,
            b: self.b - other.b,
            aa: self.aa - other.aa,
            bb: self.bb - other.bb,
            ab: self.ab - other.ab,
        }
    }

    fn add_cell(&mut self, count: u64, a_bin: usize, b_bin: usize, bins: usize) {
        let count = count as f64;
        if count == 0.0 {
            return;
        }
        let a = (a_bin as f64 + 0.5) / bins as f64;
        let b = (b_bin as f64 + 0.5) / bins as f64;
        self.count += count;
        self.a += a * count;
        self.b += b * count;
        self.aa += a * a * count;
        self.bb += b * b * count;
        self.ab += a * b * count;
    }

    fn pearson(self) -> Option<f64> {
        if self.count < MINIMUM {
            return None;
        }
        let left = self.count * self.aa - self.a * self.a;
        let right = self.count * self.bb - self.b * self.b;
        if left <= 0.0 || right <= 0.0 {
            return None;
        }
        Some(((self.count * self.ab - self.a * self.b) / (left * right).sqrt()).clamp(-1.0, 1.0))
    }
}

pub(super) fn costes(
    bins: usize,
    joint: &[u64],
    total: ColocMoments,
) -> Option<ColocThresholdRange> {
    let [sum_a, sum_b, sum_aa, sum_bb, sum_ab] = total.sums();
    let count = total.voxels() as f64;
    let mean_a = sum_a / count;
    let mean_b = sum_b / count;
    let var_a = sum_aa / count - mean_a * mean_a;
    let var_b = sum_bb / count - mean_b * mean_b;
    let covariance = sum_ab / count - mean_a * mean_b;
    if !covariance.is_finite() || covariance <= 0.0 {
        return None;
    }
    let spread = var_b - var_a;
    let slope =
        (spread + (spread * spread + 4.0 * covariance * covariance).sqrt()) / (2.0 * covariance);
    let intercept = mean_b - slope * mean_a;
    if !slope.is_finite() || slope <= 0.0 || !intercept.is_finite() {
        return None;
    }
    if total.pearson().is_none_or(|pearson| pearson <= ZERO) {
        return None;
    }
    let mut totals = Sums::default();
    for b_bin in 0..bins {
        for a_bin in 0..bins {
            totals.add_cell(joint[a_bin + bins * b_bin], a_bin, b_bin, bins);
        }
    }
    let along_a = slope < 1.0;
    let mut above = Sums::default();
    let (mut cut_a, mut cut_b) = (bins, bins);
    let mut high: Option<ColocThresholds> = None;
    let mut low: Option<ColocThresholds> = None;
    for step in (0..bins).rev() {
        let threshold = (step as f64 + 0.5) / bins as f64;
        let (next_a, next_b) = if along_a {
            (step, cut(slope * threshold + intercept, bins))
        } else {
            (cut((threshold - intercept) / slope, bins), step)
        };
        for a_bin in next_a..cut_a {
            for b_bin in next_b..bins {
                above.add_cell(joint[a_bin + bins * b_bin], a_bin, b_bin, bins);
            }
        }
        for b_bin in next_b..cut_b {
            for a_bin in cut_a..bins {
                above.add_cell(joint[a_bin + bins * b_bin], a_bin, b_bin, bins);
            }
        }
        cut_a = next_a;
        cut_b = next_b;
        let Some(pearson) = totals.minus(above).pearson() else {
            continue;
        };
        let thresholds = pair(bins, along_a, step, slope, intercept);
        if pearson <= ZERO {
            if high.is_none() {
                high = Some(thresholds);
            }
            low = Some(thresholds);
        } else if high.is_some() {
            break;
        }
    }
    let (Some(high), Some(low)) = (high, low) else {
        return None;
    };
    Some(ColocThresholdRange::new(low, high))
}

fn cut(unit: f64, bins: usize) -> usize {
    let position = unit * bins as f64 - 0.5;
    if position <= 0.0 {
        0
    } else if position >= bins as f64 {
        bins
    } else {
        position.ceil() as usize
    }
}

fn pair(bins: usize, along_a: bool, step: usize, slope: f64, intercept: f64) -> ColocThresholds {
    let threshold = (step as f64 + 0.5) / bins as f64;
    let (a, b) = if along_a {
        (threshold, slope * threshold + intercept)
    } else {
        ((threshold - intercept) / slope, threshold)
    };
    ColocThresholds::new(a.clamp(0.0, 1.0) as f32, b.clamp(0.0, 1.0) as f32)
}
