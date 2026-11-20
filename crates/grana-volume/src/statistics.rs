use std::collections::BTreeSet;

use crate::{Intensity, VoxelData, VoxelSemantics};

const LABEL_LIMIT: u32 = 256;

const DISTINCT_LIMIT: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Statistics {
    intensity: Intensity,
    distinct: u32,
    semantics: VoxelSemantics,
}

impl Statistics {
    pub fn of(intensity: Intensity, distinct: u32) -> Self {
        assert!(distinct > 0, "a census counts at least one value");
        Self {
            intensity,
            distinct,
            semantics: infer(intensity, distinct),
        }
    }

    pub fn declared(intensity: Intensity, semantics: VoxelSemantics) -> Self {
        Self {
            intensity,
            distinct: 0,
            semantics,
        }
    }

    pub fn census(data: &VoxelData) -> Self {
        let mut values = BTreeSet::new();
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        data.for_each_value(|value| {
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
            return Self::of(Intensity::new(0.0, 0.0), 1);
        }
        let distinct = u32::try_from(values.len()).unwrap_or(u32::MAX).max(1);
        Self::of(Intensity::new(minimum as f32, maximum as f32), distinct)
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
        }
    }
}

fn infer(intensity: Intensity, distinct: u32) -> VoxelSemantics {
    let binary = distinct <= 2 && intensity.min() >= 0.0 && intensity.max() <= 1.0;
    if binary {
        return VoxelSemantics::Presence;
    }
    if distinct <= LABEL_LIMIT {
        return VoxelSemantics::Label;
    }
    VoxelSemantics::Scalar
}
