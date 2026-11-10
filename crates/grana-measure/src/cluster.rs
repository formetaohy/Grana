use crate::math::{student_quantile, student_two_sided};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterSample {
    objects: usize,
    mean: f64,
    deviation: Option<f64>,
}

impl ClusterSample {
    pub fn single(value: f64) -> Option<Self> {
        Self::from_summary(1, value, None)
    }

    pub fn of(values: &[f64]) -> Option<Self> {
        if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let deviation = (values.len() > 1).then(|| {
            let variance = values
                .iter()
                .map(|value| (value - mean) * (value - mean))
                .sum::<f64>()
                / (values.len() - 1) as f64;
            variance.max(0.0).sqrt()
        });
        Self::from_summary(values.len(), mean, deviation)
    }

    pub fn from_summary(objects: usize, mean: f64, deviation: Option<f64>) -> Option<Self> {
        if objects == 0
            || !mean.is_finite()
            || deviation.is_some_and(|deviation| !deviation.is_finite() || deviation < 0.0)
        {
            return None;
        }
        Some(Self {
            objects,
            mean,
            deviation,
        })
    }

    pub fn objects(self) -> usize {
        self.objects
    }

    pub fn mean(self) -> f64 {
        self.mean
    }

    pub fn deviation(self) -> Option<f64> {
        self.deviation
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusteredWelch {
    samples_a: usize,
    samples_b: usize,
    clusters_a: usize,
    clusters_b: usize,
    mean_a: f64,
    mean_b: f64,
    deviation_a: Option<f64>,
    deviation_b: Option<f64>,
    difference: f64,
    error: f64,
    t: f64,
    df: f64,
    p: f64,
}

impl ClusteredWelch {
    pub fn of(a: &[ClusterSample], b: &[ClusterSample]) -> Option<Self> {
        if a.len() < 2 || b.len() < 2 {
            return None;
        }
        let samples_a = total(a);
        let samples_b = total(b);
        if samples_a < 2 || samples_b < 2 {
            return None;
        }
        let mean_a = weighted_mean(a);
        let mean_b = weighted_mean(b);
        let variance_a = cluster_variance(a, mean_a);
        let variance_b = cluster_variance(b, mean_b);
        let difference = mean_a - mean_b;
        let error = variance_a + variance_b;
        let df = degrees_of_freedom(a, b, variance_a, variance_b, error);
        let t = if error > 0.0 {
            difference / error.sqrt()
        } else if difference == 0.0 {
            0.0
        } else {
            f64::INFINITY.copysign(difference)
        };
        let p = student_two_sided(t, df);
        let values = [mean_a, mean_b, difference, error, df, p];
        if values.iter().any(|value| !value.is_finite()) || error < 0.0 {
            return None;
        }
        Some(Self {
            samples_a,
            samples_b,
            clusters_a: a.len(),
            clusters_b: b.len(),
            mean_a,
            mean_b,
            deviation_a: pooled_deviation(a, mean_a),
            deviation_b: pooled_deviation(b, mean_b),
            difference,
            error,
            t,
            df,
            p,
        })
    }

    pub fn samples_a(&self) -> usize {
        self.samples_a
    }

    pub fn samples_b(&self) -> usize {
        self.samples_b
    }

    pub fn clusters_a(&self) -> usize {
        self.clusters_a
    }

    pub fn clusters_b(&self) -> usize {
        self.clusters_b
    }

    pub fn mean_a(&self) -> f64 {
        self.mean_a
    }

    pub fn mean_b(&self) -> f64 {
        self.mean_b
    }

    pub fn deviation_a(&self) -> Option<f64> {
        self.deviation_a
    }

    pub fn deviation_b(&self) -> Option<f64> {
        self.deviation_b
    }

    pub fn difference(&self) -> f64 {
        self.difference
    }

    pub fn error(&self) -> f64 {
        self.error.sqrt()
    }

    pub fn t(&self) -> f64 {
        self.t
    }

    pub fn df(&self) -> f64 {
        self.df
    }

    pub fn p(&self) -> f64 {
        self.p.clamp(0.0, 1.0)
    }

    pub fn interval(&self, level: f64) -> Option<(f64, f64)> {
        assert!(
            level.is_finite() && level > 0.0 && level < 1.0,
            "a confidence level lies inside the unit interval"
        );
        let error = self.error();
        if !error.is_finite() {
            return None;
        }
        let t = student_quantile(0.5 + 0.5 * level, self.df);
        Some((self.difference - t * error, self.difference + t * error))
    }

    pub fn design_effect(&self) -> Option<f64> {
        let deviation_a = self.deviation_a?;
        let deviation_b = self.deviation_b?;
        let independent = (deviation_a * deviation_a / self.samples_a as f64
            + deviation_b * deviation_b / self.samples_b as f64)
            .sqrt();
        (independent > 0.0 && self.error > 0.0).then(|| (self.error() / independent).powi(2))
    }
}

fn total(samples: &[ClusterSample]) -> usize {
    samples.iter().map(|sample| sample.objects()).sum()
}

fn weighted_mean(samples: &[ClusterSample]) -> f64 {
    let total = total(samples) as f64;
    samples
        .iter()
        .map(|sample| sample.objects() as f64 * sample.mean())
        .sum::<f64>()
        / total
}

fn cluster_variance(samples: &[ClusterSample], mean: f64) -> f64 {
    let total = total(samples) as f64;
    let between = samples
        .iter()
        .map(|sample| {
            let offset = sample.objects() as f64 * (sample.mean() - mean);
            offset * offset
        })
        .sum::<f64>();
    let clusters = samples.len() as f64;
    between * clusters / (clusters - 1.0) / (total * total)
}

fn degrees_of_freedom(
    a: &[ClusterSample],
    b: &[ClusterSample],
    variance_a: f64,
    variance_b: f64,
    error: f64,
) -> f64 {
    let term_a = variance_a * variance_a / (a.len() - 1) as f64;
    let term_b = variance_b * variance_b / (b.len() - 1) as f64;
    let denominator = term_a + term_b;
    if error > 0.0 && denominator > 0.0 {
        error * error / denominator
    } else {
        (a.len() + b.len() - 2) as f64
    }
}

fn pooled_deviation(samples: &[ClusterSample], mean: f64) -> Option<f64> {
    let total = total(samples);
    if total < 2 {
        return None;
    }
    let within = samples
        .iter()
        .filter_map(|sample| {
            sample
                .deviation()
                .map(|deviation| (sample.objects() - 1) as f64 * deviation * deviation)
        })
        .sum::<f64>();
    let between = samples
        .iter()
        .map(|sample| {
            let offset = sample.mean() - mean;
            sample.objects() as f64 * offset * offset
        })
        .sum::<f64>();
    Some(((within + between) / (total - 1) as f64).max(0.0).sqrt())
}
