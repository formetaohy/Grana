use crate::object::{ParamField, ParamScale};

const CELLPROB_MINIMUM: f32 = -10.0;
const CELLPROB_MAXIMUM: f32 = 10.0;
const FLOW_MINIMUM: f32 = 0.0;
const FLOW_MAXIMUM: f32 = 1.0;
const SIZE_MINIMUM: f32 = 1.0;
const SIZE_MAXIMUM: f32 = 10_000.0;
const TILE_MAXIMUM: f32 = 192.0;
const SLICE_MINIMUM: f32 = 0.0;
const SLICE_MAXIMUM: f32 = 1.0;
const CELLPROB_DEFAULT: f32 = 0.0;
const FLOW_DEFAULT: f32 = 0.4;
const SIZE_DEFAULT: f32 = 15.0;
const TILE_DEFAULT: f32 = 32.0;
const SLICE_DEFAULT: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentModel {
    Cpsam,
    #[default]
    CpsamV2,
}

impl SegmentModel {
    pub const ALL: [Self; 2] = [Self::CpsamV2, Self::Cpsam];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cpsam => "cpsam",
            Self::CpsamV2 => "cpsam_v2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpsam => "Cellpose-SAM v1",
            Self::CpsamV2 => "Cellpose-SAM v2",
        }
    }

    pub fn index(self) -> u8 {
        match self {
            Self::Cpsam => 0,
            Self::CpsamV2 => 1,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Cpsam),
            1 => Some(Self::CpsamV2),
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|model| model.name() == name)
    }
}

pub const SEGMENT_FIELDS: [ParamField; 5] = [
    ParamField {
        label: "cell probability",
        suffix: "",
        minimum: ParamScale::Absolute(CELLPROB_MINIMUM),
        maximum: ParamScale::Absolute(CELLPROB_MAXIMUM),
        logarithmic: false,
    },
    ParamField {
        label: "flow error",
        suffix: "",
        minimum: ParamScale::Absolute(FLOW_MINIMUM),
        maximum: ParamScale::Absolute(FLOW_MAXIMUM),
        logarithmic: false,
    },
    ParamField {
        label: "minimum size",
        suffix: " px",
        minimum: ParamScale::Absolute(SIZE_MINIMUM),
        maximum: ParamScale::Absolute(SIZE_MAXIMUM),
        logarithmic: true,
    },
    ParamField {
        label: "tile overlap",
        suffix: " px",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(TILE_MAXIMUM),
        logarithmic: false,
    },
    ParamField {
        label: "slice overlap",
        suffix: "",
        minimum: ParamScale::Absolute(SLICE_MINIMUM),
        maximum: ParamScale::Absolute(SLICE_MAXIMUM),
        logarithmic: false,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentParams {
    cellprob_threshold: f32,
    flow_threshold: f32,
    minimum_size: u32,
    tile_overlap: u32,
    slice_overlap: f32,
    model: SegmentModel,
}

impl Default for SegmentParams {
    fn default() -> Self {
        Self {
            cellprob_threshold: CELLPROB_DEFAULT,
            flow_threshold: FLOW_DEFAULT,
            minimum_size: SIZE_DEFAULT as u32,
            tile_overlap: TILE_DEFAULT as u32,
            slice_overlap: SLICE_DEFAULT,
            model: SegmentModel::default(),
        }
    }
}

impl SegmentParams {
    pub const FIELD_COUNT: usize = SEGMENT_FIELDS.len();

    pub fn with_settings(
        cellprob_threshold: f32,
        flow_threshold: f32,
        minimum_size: u32,
        tile_overlap: u32,
        slice_overlap: f32,
    ) -> Self {
        let mut params = Self::default();
        params.set_cellprob_threshold(cellprob_threshold);
        params.set_flow_threshold(flow_threshold);
        params.set_minimum_size(minimum_size);
        params.set_tile_overlap(tile_overlap);
        params.set_slice_overlap(slice_overlap);
        params
    }

    pub fn cellprob_threshold(&self) -> f32 {
        self.cellprob_threshold
    }

    pub fn flow_threshold(&self) -> f32 {
        self.flow_threshold
    }

    pub fn minimum_size(&self) -> u32 {
        self.minimum_size
    }

    pub fn tile_overlap(&self) -> u32 {
        self.tile_overlap
    }

    pub fn slice_overlap(&self) -> f32 {
        self.slice_overlap
    }

    pub fn model(&self) -> SegmentModel {
        self.model
    }

    pub fn with_model(mut self, model: SegmentModel) -> Self {
        self.model = model;
        self
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.cellprob_threshold,
            1 => self.flow_threshold,
            2 => self.minimum_size as f32,
            3 => self.tile_overlap as f32,
            4 => self.slice_overlap,
            _ => panic!("a segmentation carries five parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_cellprob_threshold(value),
            1 => self.set_flow_threshold(value),
            2 => self.set_minimum_size(value.round() as u32),
            3 => self.set_tile_overlap(value.round() as u32),
            4 => self.set_slice_overlap(value),
            _ => panic!("a segmentation carries five parameters"),
        }
    }

    pub fn set_cellprob_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite() && (CELLPROB_MINIMUM..=CELLPROB_MAXIMUM).contains(&threshold),
            "a cell probability threshold lies within the model logits"
        );
        self.cellprob_threshold = threshold;
    }

    pub fn set_flow_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite() && (FLOW_MINIMUM..=FLOW_MAXIMUM).contains(&threshold),
            "a flow error threshold is a unit fraction"
        );
        self.flow_threshold = threshold;
    }

    pub fn set_minimum_size(&mut self, size: u32) {
        assert!(
            size <= SIZE_MAXIMUM as u32,
            "a segmented object holds a bounded number of pixels"
        );
        self.minimum_size = size;
    }

    pub fn set_tile_overlap(&mut self, overlap: u32) {
        assert!(
            overlap <= TILE_MAXIMUM as u32,
            "a segmentation tile overlap is smaller than the model side"
        );
        self.tile_overlap = overlap;
    }

    pub fn set_slice_overlap(&mut self, overlap: f32) {
        assert!(
            overlap.is_finite() && (SLICE_MINIMUM..=SLICE_MAXIMUM).contains(&overlap),
            "a slice overlap is a unit fraction"
        );
        self.slice_overlap = overlap;
    }
}
