#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleStatistics {
    mean: f32,
    minimum: f32,
    maximum: f32,
    deviation: f32,
    sum: f64,
}

impl SampleStatistics {
    pub fn new(mean: f32, minimum: f32, maximum: f32, deviation: f32, sum: f64) -> Self {
        assert!(
            [mean, minimum, maximum, deviation]
                .iter()
                .all(|value| value.is_finite()),
            "intensity statistics are finite"
        );
        assert!(
            minimum <= mean && mean <= maximum,
            "intensity statistics are ordered"
        );
        assert!(deviation >= 0.0, "an intensity deviation is non negative");
        assert!(
            sum.is_finite() && sum >= 0.0,
            "an intensity sum is a non negative total"
        );
        Self {
            mean,
            minimum,
            maximum,
            deviation,
            sum,
        }
    }

    pub fn mean(self) -> f32 {
        self.mean
    }

    pub fn minimum(self) -> f32 {
        self.minimum
    }

    pub fn maximum(self) -> f32 {
        self.maximum
    }

    pub fn deviation(self) -> f32 {
        self.deviation
    }

    pub fn sum(self) -> f64 {
        self.sum
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleAccumulator {
    count: u64,
    mean: f64,
    squared: f64,
    minimum: f32,
    maximum: f32,
    sum: f64,
}

impl SampleAccumulator {
    pub fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            squared: 0.0,
            minimum: f32::INFINITY,
            maximum: f32::NEG_INFINITY,
            sum: 0.0,
        }
    }

    pub fn push(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        self.count += 1;
        self.sum += f64::from(value);
        self.minimum = self.minimum.min(value);
        self.maximum = self.maximum.max(value);
        let delta = f64::from(value) - self.mean;
        self.mean += delta / self.count as f64;
        self.squared += delta * (f64::from(value) - self.mean);
    }

    pub fn merge(&mut self, other: &Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        let total = self.count + other.count;
        let delta = other.mean - self.mean;
        self.mean += delta * other.count as f64 / total as f64;
        self.squared +=
            other.squared + delta * delta * (self.count as f64 * other.count as f64 / total as f64);
        self.count = total;
        self.sum += other.sum;
        self.minimum = self.minimum.min(other.minimum);
        self.maximum = self.maximum.max(other.maximum);
    }

    pub fn statistics(self) -> SampleStatistics {
        if self.count == 0 {
            return SampleStatistics::new(0.0, 0.0, 0.0, 0.0, 0.0);
        }
        let mean = self
            .mean
            .clamp(f64::from(self.minimum), f64::from(self.maximum)) as f32;
        let deviation = (self.squared / self.count as f64).max(0.0).sqrt() as f32;
        SampleStatistics::new(mean, self.minimum, self.maximum, deviation, self.sum)
    }

    pub(crate) fn parts(self) -> AccumulatorParts {
        AccumulatorParts {
            count: self.count,
            mean: self.mean,
            squared: self.squared,
            minimum: self.minimum,
            maximum: self.maximum,
            sum: self.sum,
        }
    }

    pub(crate) fn restore(parts: AccumulatorParts) -> Option<Self> {
        if parts.count == 0 {
            let empty = parts.mean == 0.0
                && parts.squared == 0.0
                && parts.sum == 0.0
                && parts.minimum == f32::INFINITY
                && parts.maximum == f32::NEG_INFINITY;
            return empty.then(Self::new);
        }
        let ordered = f64::from(parts.minimum) <= parts.mean
            && parts.mean <= f64::from(parts.maximum)
            && parts.mean.is_finite()
            && parts.squared.is_finite()
            && parts.squared >= 0.0
            && parts.sum.is_finite()
            && parts.sum >= 0.0
            && parts.minimum.is_finite()
            && parts.maximum.is_finite();
        ordered.then_some(Self {
            count: parts.count,
            mean: parts.mean,
            squared: parts.squared,
            minimum: parts.minimum,
            maximum: parts.maximum,
            sum: parts.sum,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AccumulatorParts {
    pub(crate) count: u64,
    pub(crate) mean: f64,
    pub(crate) squared: f64,
    pub(crate) minimum: f32,
    pub(crate) maximum: f32,
    pub(crate) sum: f64,
}

impl Default for SampleAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntensityTable {
    channels: usize,
    values: Vec<SampleStatistics>,
}

impl IntensityTable {
    pub fn new(channels: usize, values: Vec<SampleStatistics>) -> Self {
        assert!(
            channels > 0,
            "an intensity table measures at least one channel"
        );
        assert_eq!(
            values.len() % channels,
            0,
            "every object is measured in every channel"
        );
        Self { channels, values }
    }

    pub fn empty(channels: usize) -> Self {
        Self::new(channels, Vec::new())
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn objects(&self) -> usize {
        self.values.len() / self.channels
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn object(&self, object: usize, channel: usize) -> SampleStatistics {
        assert!(
            object < self.objects() && channel < self.channels,
            "the measured object lies inside the intensity table"
        );
        self.values[object * self.channels + channel]
    }

    pub fn without(&self, object: usize) -> Self {
        assert!(
            object < self.objects(),
            "the removed object is measured by the intensity table"
        );
        let mut values = self.values.clone();
        values.drain(object * self.channels..(object + 1) * self.channels);
        Self::new(self.channels, values)
    }

    pub fn retain(&self, keep: &[bool]) -> Self {
        assert_eq!(
            keep.len(),
            self.objects(),
            "the kept objects are measured by the intensity table"
        );
        let mut values =
            Vec::with_capacity(keep.iter().filter(|kept| **kept).count() * self.channels);
        for (object, kept) in keep.iter().enumerate() {
            if *kept {
                values.extend_from_slice(
                    &self.values[object * self.channels..(object + 1) * self.channels],
                );
            }
        }
        Self::new(self.channels, values)
    }
}
