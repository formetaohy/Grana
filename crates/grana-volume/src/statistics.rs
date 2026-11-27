use std::collections::BTreeSet;

use crate::{Histogram, Intensity, NoData, VoxelData, VoxelSemantics};

const DISTINCT_LIMIT: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    statistics: Statistics,
    histogram: Histogram,
}

impl Measured {
    pub(crate) fn of(statistics: Statistics, histogram: Histogram) -> Self {
        Self {
            statistics,
            histogram,
        }
    }

    pub fn statistics(&self) -> Statistics {
        self.statistics
    }

    pub fn histogram(&self) -> &Histogram {
        &self.histogram
    }

    pub fn background(&self) -> f32 {
        self.statistics.background()
    }

    pub fn into_parts(self) -> (Statistics, Histogram) {
        (self.statistics, self.histogram)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Statistics {
    intensity: Intensity,
    distinct: u32,
    semantics: VoxelSemantics,
    background: f32,
}

impl Statistics {
    pub fn of(intensity: Intensity, distinct: u32) -> Self {
        assert!(distinct > 0, "a census counts at least one value");
        Self {
            intensity,
            distinct,
            semantics: infer(intensity, distinct),
            background: intensity.min(),
        }
    }

    pub fn declared(intensity: Intensity, semantics: VoxelSemantics) -> Self {
        Self {
            intensity,
            distinct: 0,
            semantics,
            background: intensity.min(),
        }
    }

    pub fn census(data: &VoxelData, no_data: Option<NoData>) -> Measured {
        let mut values = BTreeSet::new();
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        data.for_each_present_value(no_data, |value| {
            let value = f64::from(value);
            if !value.is_finite() {
                return;
            }
            minimum = minimum.min(value);
            maximum = maximum.max(value);
            if values.len() <= DISTINCT_LIMIT {
                values.insert(value.to_bits());
            }
        });
        if minimum > maximum {
            let statistics = Self::of(Intensity::new(0.0, 0.0), 1);
            let histogram = Histogram::over(statistics.intensity());
            return Measured {
                statistics,
                histogram,
            };
        }
        let distinct = u32::try_from(values.len()).unwrap_or(u32::MAX).max(1);
        let intensity = Intensity::new(minimum as f32, maximum as f32);
        let mut histogram = Histogram::over(intensity);
        data.for_each_present_value(no_data, |value| histogram.add(value));
        let statistics = Self::of(intensity, distinct).with_background(histogram.background());
        Measured {
            statistics,
            histogram,
        }
    }

    pub fn intensity(self) -> Intensity {
        self.intensity
    }

    pub fn distinct(self) -> u32 {
        self.distinct
    }

    pub fn semantics(self) -> VoxelSemantics {
        self.semantics
    }

    pub fn background(self) -> f32 {
        self.background
    }

    pub fn with_background(mut self, background: f32) -> Self {
        assert!(
            background.is_finite(),
            "a channel background is a finite value"
        );
        self.background = background.clamp(self.intensity.min(), self.intensity.max());
        self
    }

    pub fn folded(self, other: Self) -> Self {
        let intensity = Intensity::new(
            self.intensity.min().min(other.intensity.min()),
            self.intensity.max().max(other.intensity.max()),
        );
        let semantics = if self.semantics == other.semantics {
            self.semantics
        } else {
            VoxelSemantics::Scalar
        };
        Self {
            intensity,
            distinct: self.distinct.max(other.distinct),
            semantics,
            background: self.background.min(other.background),
        }
    }
}

fn infer(intensity: Intensity, distinct: u32) -> VoxelSemantics {
    let binary = distinct <= 2 && intensity.min() >= 0.0 && intensity.max() <= 1.0;
    if binary {
        VoxelSemantics::Presence
    } else {
        VoxelSemantics::Scalar
    }
}
