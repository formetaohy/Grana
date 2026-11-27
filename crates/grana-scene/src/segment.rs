use crate::object::{ParamField, ParamScale};

const FOREGROUND_MINIMUM: f32 = 0.0;
const FOREGROUND_MAXIMUM: f32 = 1.0;
const DENSITY_MINIMUM: f32 = 0.0;
const DENSITY_MAXIMUM: f32 = 10_000.0;
const VOLUME_MINIMUM: f32 = 0.001;
const VOLUME_MAXIMUM: f32 = 1_000.0;
const FOREGROUND_DEFAULT: f32 = 0.5;
const DENSITY_2D_DEFAULT: f32 = 30.0;
const DENSITY_3D_DEFAULT: f32 = 80.0;
const VOLUME_2D_DEFAULT: f32 = 0.0011;
const VOLUME_3D_DEFAULT: f32 = 0.01;

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
        label: "minimum volume",
        suffix: " µm³",
        minimum: ParamScale::Absolute(VOLUME_MINIMUM),
        maximum: ParamScale::Absolute(VOLUME_MAXIMUM),
        logarithmic: true,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentParams {
    foreground_threshold: f32,
    density_threshold: f32,
    minimum_volume: f32,
}

impl SegmentParams {
    pub const FIELD_COUNT: usize = SEGMENT_FIELDS.len();

    pub fn new(depth: u32) -> Self {
        if depth > 1 {
            Self {
                foreground_threshold: FOREGROUND_DEFAULT,
                density_threshold: DENSITY_3D_DEFAULT,
                minimum_volume: VOLUME_3D_DEFAULT,
            }
        } else {
            Self {
                foreground_threshold: FOREGROUND_DEFAULT,
                density_threshold: DENSITY_2D_DEFAULT,
                minimum_volume: VOLUME_2D_DEFAULT,
            }
        }
    }

    pub fn with_settings(
        foreground_threshold: f32,
        density_threshold: f32,
        minimum_volume: f32,
    ) -> Self {
        let mut params = Self::new(1);
        params.set_foreground_threshold(foreground_threshold);
        params.set_density_threshold(density_threshold);
        params.set_minimum_volume(minimum_volume);
        params
    }

    pub fn foreground_threshold(&self) -> f32 {
        self.foreground_threshold
    }

    pub fn density_threshold(&self) -> f32 {
        self.density_threshold
    }

    pub fn minimum_volume(&self) -> f32 {
        self.minimum_volume
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.foreground_threshold,
            1 => self.density_threshold,
            2 => self.minimum_volume,
            _ => panic!("a segmentation carries three parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_foreground_threshold(value),
            1 => self.set_density_threshold(value),
            2 => self.set_minimum_volume(value),
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

    pub fn set_minimum_volume(&mut self, volume: f32) {
        assert!(
            volume.is_finite() && (VOLUME_MINIMUM..=VOLUME_MAXIMUM).contains(&volume),
            "a segmented object covers a positive physical volume"
        );
        self.minimum_volume = volume;
    }
}
