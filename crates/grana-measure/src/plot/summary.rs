use crate::sample::Sample;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    count: u64,
    mean: f64,
    deviation: f64,
    minimum: f64,
    maximum: f64,
    median: f64,
    lower: f64,
    upper: f64,
    mad: f64,
}

impl Summary {
    pub fn count(self) -> u64 {
        self.count
    }

    pub fn mean(self) -> f64 {
        self.mean
    }

    pub fn deviation(self) -> f64 {
        self.deviation
    }

    pub fn minimum(self) -> f64 {
        self.minimum
    }

    pub fn maximum(self) -> f64 {
        self.maximum
    }

    pub fn median(self) -> f64 {
        self.median
    }

    pub fn lower_quartile(self) -> f64 {
        self.lower
    }

    pub fn upper_quartile(self) -> f64 {
        self.upper
    }

    pub fn mad(self) -> f64 {
        self.mad
    }
}

pub(crate) struct Moments {
    values: Vec<f32>,
}

impl Moments {
    pub(crate) fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub(crate) fn push(&mut self, value: f32) {
        if value.is_finite() {
            self.values.push(value);
        }
    }

    pub(crate) fn summary(self) -> Summary {
        if self.values.is_empty() {
            return Summary::default();
        }
        let sample = Sample::new(self.values.into_iter().map(f64::from).collect())
            .expect("a moment holds a finite value");
        Summary {
            count: sample.len() as u64,
            mean: sample.mean(),
            deviation: sample.deviation().unwrap_or(0.0),
            minimum: sample.minimum(),
            maximum: sample.maximum(),
            median: sample.median(),
            lower: sample.quantile(25.0),
            upper: sample.quantile(75.0),
            mad: sample.mad(),
        }
    }
}
