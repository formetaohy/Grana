use crate::Result;
use crate::error::Error;

const MINIMUM_VOLUME_LIMIT: f32 = 1_000.0;
const ROUNDING_TOLERANCE: f64 = 1.0e-6;
const MAXIMUM_CELLS: f64 = usize::MAX as f64;
const DENSITY_LIMIT: f32 = 10_000.0;
const FOREGROUND_2D: f32 = 0.5;
const FOREGROUND_3D: f32 = 0.5;
const DENSITY_2D: f32 = 30.0;
const DENSITY_3D: f32 = 80.0;
const VOLUME_2D: f32 = 0.0011;
const VOLUME_3D: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    foreground_threshold: f32,
    density_threshold: f32,
    minimum_volume: f32,
}

impl Settings {
    pub fn new(
        foreground_threshold: f32,
        density_threshold: f32,
        minimum_volume: f32,
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
        if !minimum_volume.is_finite() || !(0.0..=MINIMUM_VOLUME_LIMIT).contains(&minimum_volume) {
            return Err(Error::input(
                "the minimum volume must be a finite value inside the object scale",
            ));
        }
        Ok(Self {
            foreground_threshold,
            density_threshold,
            minimum_volume,
        })
    }

    pub fn for_depth(depth: u32) -> Self {
        if depth > 1 {
            Self {
                foreground_threshold: FOREGROUND_3D,
                density_threshold: DENSITY_3D,
                minimum_volume: VOLUME_3D,
            }
        } else {
            Self {
                foreground_threshold: FOREGROUND_2D,
                density_threshold: DENSITY_2D,
                minimum_volume: VOLUME_2D,
            }
        }
    }

    pub fn foreground_threshold(self) -> f32 {
        self.foreground_threshold
    }

    pub fn density_threshold(self) -> f32 {
        self.density_threshold
    }

    pub fn minimum_volume(self) -> f32 {
        self.minimum_volume
    }

    pub fn minimum_voxel_count(self, cell_measure: f64) -> usize {
        assert!(
            cell_measure.is_finite() && cell_measure > 0.0,
            "a segmented cell spans a positive physical volume"
        );
        let ratio = f64::from(self.minimum_volume) / cell_measure;
        let rounded = (ratio + ROUNDING_TOLERANCE * ratio.max(1.0)).floor();
        if rounded >= MAXIMUM_CELLS {
            return usize::MAX;
        }
        rounded as usize
    }
}
