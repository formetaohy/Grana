use crate::object::{ParamField, ParamScale};

const FOREGROUND_MINIMUM: f32 = 0.0;
const FOREGROUND_MAXIMUM: f32 = 1.0;
const DENSITY_MINIMUM: f32 = 0.0;
const DENSITY_MAXIMUM: f32 = 10_000.0;
const SIZE_MINIMUM: f32 = 1.0;
const SIZE_MAXIMUM: f32 = 10_000.0;
const FOREGROUND_DEFAULT: f32 = 0.5;
const DENSITY_2D_DEFAULT: f32 = 30.0;
const DENSITY_3D_DEFAULT: f32 = 80.0;
const SIZE_2D_DEFAULT: u32 = 50;
const SIZE_3D_DEFAULT: u32 = 500;

pub const SEGMENT_FIELDS: [ParamField; 3] = [
    ParamField {
        label: "foreground",
        suffix: "",
        minimum: ParamScale::Absolute(FOREGROUND_MINIMUM),
        maximum: ParamScale::Absolute(FOREGROUND_MAXIMUM),
        logarithmic: false,
    },
    ParamField {
        label: "density",
        suffix: "",
        minimum: ParamScale::Absolute(DENSITY_MINIMUM),
        maximum: ParamScale::Absolute(DENSITY_MAXIMUM),
        logarithmic: false,
    },
    ParamField {
        label: "minimum size",
        suffix: " vox",
        minimum: ParamScale::Absolute(SIZE_MINIMUM),
        maximum: ParamScale::Absolute(SIZE_MAXIMUM),
        logarithmic: true,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentParams {
    foreground_threshold: f32,
    density_threshold: f32,
    minimum_size: u32,
}

impl SegmentParams {
    pub const FIELD_COUNT: usize = SEGMENT_FIELDS.len();

    pub fn new(depth: u32) -> Self {
        if depth > 1 {
            Self {
                foreground_threshold: FOREGROUND_DEFAULT,
                density_threshold: DENSITY_3D_DEFAULT,
                minimum_size: SIZE_3D_DEFAULT,
            }
        } else {
            Self {
                foreground_threshold: FOREGROUND_DEFAULT,
                density_threshold: DENSITY_2D_DEFAULT,
                minimum_size: SIZE_2D_DEFAULT,
            }
        }
    }

    pub fn with_settings(
        foreground_threshold: f32,
        density_threshold: f32,
        minimum_size: u32,
    ) -> Self {
        let mut params = Self::new(1);
        params.set_foreground_threshold(foreground_threshold);
        params.set_density_threshold(density_threshold);
        params.set_minimum_size(minimum_size);
        params
    }

    pub fn foreground_threshold(&self) -> f32 {
        self.foreground_threshold
    }

    pub fn density_threshold(&self) -> f32 {
        self.density_threshold
    }

    pub fn minimum_size(&self) -> u32 {
        self.minimum_size
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.foreground_threshold,
            1 => self.density_threshold,
            2 => self.minimum_size as f32,
            _ => panic!("a segmentation carries three parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_foreground_threshold(value),
            1 => self.set_density_threshold(value),
            2 => self.set_minimum_size(value.round() as u32),
            _ => panic!("a segmentation carries three parameters"),
        }
    }

    pub fn set_foreground_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite() && (FOREGROUND_MINIMUM..=FOREGROUND_MAXIMUM).contains(&threshold),
            "a foreground threshold is a unit fraction"
        );
        self.foreground_threshold = threshold;
    }

    pub fn set_density_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite() && (DENSITY_MINIMUM..=DENSITY_MAXIMUM).contains(&threshold),
            "a density threshold is a non-negative particle count"
        );
        self.density_threshold = threshold;
    }

    pub fn set_minimum_size(&mut self, size: u32) {
        assert!(
            size <= SIZE_MAXIMUM as u32,
            "a segmented object holds a bounded number of voxels"
        );
        self.minimum_size = size;
    }
}
