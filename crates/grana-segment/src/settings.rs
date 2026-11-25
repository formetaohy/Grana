use crate::Result;
use crate::error::Error;

const MINIMUM_SIZE_LIMIT: u32 = 10_000;
const DENSITY_LIMIT: f32 = 10_000.0;
const FOREGROUND_2D: f32 = 0.5;
const FOREGROUND_3D: f32 = 0.5;
const DENSITY_2D: f32 = 30.0;
const DENSITY_3D: f32 = 80.0;
const SIZE_2D: u32 = 50;
const SIZE_3D: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    foreground_threshold: f32,
    density_threshold: f32,
    minimum_size: u32,
}

impl Settings {
    pub fn new(
        foreground_threshold: f32,
        density_threshold: f32,
        minimum_size: u32,
    ) -> Result<Self> {
        if !foreground_threshold.is_finite() || !(0.0..=1.0).contains(&foreground_threshold) {
            return Err(Error::input(
                "the foreground threshold must be a finite value in the range 0..=1",
            ));
        }
        if !density_threshold.is_finite() || !(0.0..=DENSITY_LIMIT).contains(&density_threshold) {
            return Err(Error::input(
                "the density threshold must be a finite non-negative value",
            ));
        }
        if minimum_size > MINIMUM_SIZE_LIMIT {
            return Err(Error::input(
                "the minimum size exceeds the bounded object volume",
            ));
        }
        Ok(Self {
            foreground_threshold,
            density_threshold,
            minimum_size,
        })
    }

    pub fn for_depth(depth: u32) -> Self {
        if depth > 1 {
            Self {
                foreground_threshold: FOREGROUND_3D,
                density_threshold: DENSITY_3D,
                minimum_size: SIZE_3D,
            }
        } else {
            Self {
                foreground_threshold: FOREGROUND_2D,
                density_threshold: DENSITY_2D,
                minimum_size: SIZE_2D,
            }
        }
    }

    pub fn foreground_threshold(self) -> f32 {
        self.foreground_threshold
    }

    pub fn density_threshold(self) -> f32 {
        self.density_threshold
    }

    pub fn minimum_size(self) -> u32 {
        self.minimum_size
    }
}
