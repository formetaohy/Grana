use crate::math::binomial_half_cdf;
use crate::math::student_quantile;

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    values: Vec<f64>,
}

impl Sample {
    pub fn new(values: Vec<f64>) -> Option<Self> {
        let mut values = values
            .into_iter()
            .filter(|value| value.is_finite())
            .collect::<Vec<_>>();
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        Some(Self { values })
    }

    pub fn of(values: &[f64]) -> Option<Self> {
        Self::new(values.to_vec())
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn mean(&self) -> f64 {
        self.values.iter().sum::<f64>() / self.values.len() as f64
    }

    pub fn minimum(&self) -> f64 {
        self.values[0]
    }

    pub fn maximum(&self) -> f64 {
        self.values[self.values.len() - 1]
    }

    pub fn deviation(&self) -> Option<f64> {
        if self.values.len() < 2 {
            return None;
        }
        let mean = self.mean();
        let variance = self
            .values
            .iter()
            .map(|value| (value - mean) * (value - mean))
            .sum::<f64>()
            / (self.values.len() - 1) as f64;
        Some(variance.max(0.0).sqrt())
    }

    pub fn standard_error(&self) -> Option<f64> {
        self.deviation()
            .map(|deviation| deviation / (self.values.len() as f64).sqrt())
    }

    pub fn median(&self) -> f64 {
        self.quantile(50.0)
    }

    pub fn quantile(&self, percent: f64) -> f64 {
        assert!(
            percent.is_finite() && (0.0..=100.0).contains(&percent),
            "a quantile samples the closed unit interval"
        );
        if self.values.len() == 1 {
            return self.values[0];
        }
        let position = percent / 100.0 * (self.values.len() - 1) as f64;
        let low = position.floor() as usize;
        let high = position.ceil() as usize;
        let weight = position - low as f64;
        self.values[low] + (self.values[high] - self.values[low]) * weight
    }

    pub fn mad(&self) -> f64 {
        let median = self.median();
        let mut deviations = self
            .values
            .iter()
            .map(|value| (value - median).abs())
            .collect::<Vec<_>>();
        deviations.sort_by(f64::total_cmp);
        Sample::new(deviations)
            .map(|sample| sample.median())
            .expect("a sample carries one absolute deviation")
    }

    pub fn scaled_mad(&self) -> f64 {
        self.mad() * 1.482_602_218_505_602
    }

    pub fn mean_interval(&self, level: f64) -> Option<(f64, f64)> {
        assert!(
            level.is_finite() && level > 0.0 && level < 1.0,
            "a confidence level lies inside the unit interval"
        );
        if self.values.len() < 2 {
            return None;
        }
        let error = self.standard_error()?;
        let t = student_quantile(0.5 + 0.5 * level, (self.values.len() - 1) as f64);
        let mean = self.mean();
        Some((mean - t * error, mean + t * error))
    }

    pub fn median_interval(&self, level: f64) -> Option<(f64, f64)> {
        assert!(
            level.is_finite() && level > 0.0 && level < 1.0,
            "a confidence level lies inside the unit interval"
        );
        let n = self.values.len();
        if n < 2 {
            return None;
        }
        let tail = 0.5 * (1.0 - level);
        let mut order = None;
        for candidate in 1..=n / 2 {
            if binomial_half_cdf(candidate - 1, n) <= tail {
                order = Some(candidate);
            } else {
                break;
            }
        }
        let order = order?;
        Some((self.values[order - 1], self.values[n - order]))
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }
}
