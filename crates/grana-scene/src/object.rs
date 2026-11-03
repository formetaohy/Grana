use std::sync::Arc;

use grana_geometry::PackedMesh;
use grana_measure::{Measurement, Objects};
use grana_object::{
    FilamentParams, FilamentSet, IntensityTable, Shape, Spot, SpotParams, SpotSet, SurfaceFilter,
    SurfaceParams, SurfaceSet, TrackParams,
};

const SPOT_STEP: f32 = 8.0;
const SURFACE_STEP: f32 = 12.0;
const SPOT_THICKNESS: f32 = 1.0;
const SURFACE_THICKNESS: f32 = 2.0;
const MAX_GAP: u32 = 1;
const DIVISION_SCALE: f32 = 2.0;
const SPOT_DIAMETER: f32 = 4.0;
const SPOT_CONTRAST: f32 = 0.2;
const SURFACE_THRESHOLD: f32 = 0.5;
const SURFACE_GRAIN: f32 = 1.0;
const SURFACE_SPLIT: f32 = 0.4;
const SURFACE_MIN_VOLUME: f32 = 0.0;
const SURFACE_OPENING: f32 = 0.0;
const SURFACE_CLOSING: f32 = 0.0;
const SURFACE_MIN_SPHERICITY: f32 = 0.0;
const SURFACE_MIN_MEAN_INTENSITY: f32 = 0.0;
const FILAMENT_THRESHOLD: f32 = 0.5;
const FILAMENT_MIN_LENGTH: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Spots,
    Surfaces,
    Filaments,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamScale {
    Voxel(f32),
    World(f32),
    Absolute(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamField {
    pub label: &'static str,
    pub suffix: &'static str,
    pub minimum: ParamScale,
    pub maximum: ParamScale,
    pub logarithmic: bool,
}

const SPOT_FIELDS: [ParamField; 2] = [
    ParamField {
        label: "diameter",
        suffix: " µm",
        minimum: ParamScale::Voxel(1.0),
        maximum: ParamScale::Voxel(16.0),
        logarithmic: true,
    },
    ParamField {
        label: "contrast",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
];

const SURFACE_FIELDS: [ParamField; SurfaceParams::FIELD_COUNT] = [
    ParamField {
        label: "threshold",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
    ParamField {
        label: "grain",
        suffix: " µm",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Voxel(3.0),
        logarithmic: false,
    },
    ParamField {
        label: "splitting",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
    ParamField {
        label: "min volume",
        suffix: " µm³",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(200.0),
        logarithmic: false,
    },
    ParamField {
        label: "opening radius",
        suffix: " µm",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Voxel(8.0),
        logarithmic: false,
    },
    ParamField {
        label: "closing radius",
        suffix: " µm",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Voxel(8.0),
        logarithmic: false,
    },
    ParamField {
        label: "min sphericity",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
    ParamField {
        label: "min mean intensity",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
];

const FILAMENT_FIELDS: [ParamField; 2] = [
    ParamField {
        label: "threshold",
        suffix: "",
        minimum: ParamScale::Absolute(0.0),
        maximum: ParamScale::Absolute(1.0),
        logarithmic: false,
    },
    ParamField {
        label: "min length",
        suffix: " µm",
        minimum: ParamScale::Voxel(1.0),
        maximum: ParamScale::World(0.25),
        logarithmic: true,
    },
];

impl ObjectKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Spots => "Spots",
            Self::Surfaces => "Surfaces",
            Self::Filaments => "Filaments",
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::Spots => "spot",
            Self::Surfaces => "object",
            Self::Filaments => "filament",
        }
    }

    pub fn count_noun(self) -> &'static str {
        match self {
            Self::Spots => "spots",
            Self::Surfaces => "objects",
            Self::Filaments => "filaments",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Spots => "detects intensity maxima in",
            Self::Surfaces => "extracts connected surfaces of",
            Self::Filaments => "traces centerlines through",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Spots => 0,
            Self::Surfaces => 1,
            Self::Filaments => 2,
        }
    }

    pub fn trackable(self) -> bool {
        !matches!(self, Self::Filaments)
    }

    pub fn measurements(self, channels: usize) -> Vec<Measurement> {
        match self {
            Self::Spots => Measurement::spots(channels),
            Self::Surfaces => Measurement::surfaces(channels),
            Self::Filaments => Measurement::filaments(channels),
        }
    }

    pub fn fields(self) -> &'static [ParamField] {
        match self {
            Self::Spots => &SPOT_FIELDS,
            Self::Surfaces => &SURFACE_FIELDS,
            Self::Filaments => &FILAMENT_FIELDS,
        }
    }

    pub fn default_params(self, voxel: f32) -> ObjectParams {
        match self {
            Self::Spots => {
                ObjectParams::Spots(SpotParams::new(SPOT_DIAMETER * voxel, SPOT_CONTRAST))
            }
            Self::Surfaces => ObjectParams::Surfaces(SurfaceParams::new(
                SURFACE_THRESHOLD,
                SURFACE_GRAIN * voxel,
                SURFACE_SPLIT,
                SURFACE_MIN_VOLUME,
                SurfaceFilter::new(
                    SURFACE_OPENING * voxel,
                    SURFACE_CLOSING * voxel,
                    SURFACE_MIN_SPHERICITY,
                    SURFACE_MIN_MEAN_INTENSITY,
                ),
            )),
            Self::Filaments => ObjectParams::Filaments(FilamentParams::new(
                FILAMENT_THRESHOLD,
                FILAMENT_MIN_LENGTH,
            )),
        }
    }

    pub fn track_style(self, voxel: f32) -> Option<TrackStyle> {
        let (step, thickness) = match self {
            Self::Spots => (SPOT_STEP, SPOT_THICKNESS),
            Self::Surfaces => (SURFACE_STEP, SURFACE_THICKNESS),
            Self::Filaments => return None,
        };
        let step = step * voxel;
        Some(TrackStyle::new(
            TrackParams::new(step, MAX_GAP, true, DIVISION_SCALE * step),
            thickness * voxel,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackStyle {
    params: TrackParams,
    thickness: f32,
    visible: bool,
}

impl TrackStyle {
    pub fn new(params: TrackParams, thickness: f32) -> Self {
        assert!(
            thickness.is_finite() && thickness > 0.0,
            "a track tube is a positive thickness"
        );
        Self {
            params,
            thickness,
            visible: true,
        }
    }

    pub fn params(self) -> TrackParams {
        self.params
    }

    pub fn thickness(self) -> f32 {
        self.thickness
    }

    pub fn visible(self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjectParams {
    Spots(SpotParams),
    Surfaces(SurfaceParams),
    Filaments(FilamentParams),
}

impl ObjectParams {
    pub fn kind(self) -> ObjectKind {
        match self {
            Self::Spots(_) => ObjectKind::Spots,
            Self::Surfaces(_) => ObjectKind::Surfaces,
            Self::Filaments(_) => ObjectKind::Filaments,
        }
    }

    pub fn value(&self, index: usize) -> f32 {
        match self {
            Self::Spots(params) => params.field(index),
            Self::Surfaces(params) => params.field(index),
            Self::Filaments(params) => params.field(index),
        }
    }

    pub fn set_value(&mut self, index: usize, value: f32) {
        match self {
            Self::Spots(params) => params.set_field(index, value),
            Self::Surfaces(params) => params.set_field(index, value),
            Self::Filaments(params) => params.set_field(index, value),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectFrame {
    Spots(SpotSet),
    Surfaces(SurfaceSet),
    Filaments(FilamentSet),
}

impl ObjectFrame {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Spots(_) => ObjectKind::Spots,
            Self::Surfaces(_) => ObjectKind::Surfaces,
            Self::Filaments(_) => ObjectKind::Filaments,
        }
    }

    pub fn level(&self) -> usize {
        match self {
            Self::Spots(set) => set.level(),
            Self::Surfaces(set) => set.level(),
            Self::Filaments(set) => set.level(),
        }
    }

    pub fn object_count(&self) -> usize {
        match self {
            Self::Spots(set) => set.len(),
            Self::Surfaces(set) => set.object_count(),
            Self::Filaments(set) => set.object_count(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.object_count() == 0
    }

    pub fn spots(&self) -> Option<&SpotSet> {
        match self {
            Self::Spots(set) => Some(set),
            Self::Surfaces(_) | Self::Filaments(_) => None,
        }
    }

    pub fn geometry(&self) -> Result<Option<Arc<PackedMesh>>, grana_geometry::Error> {
        Ok(match self {
            Self::Spots(_) => None,
            Self::Surfaces(set) => Some(set.geometry()?),
            Self::Filaments(set) => Some(set.geometry()?),
        })
    }

    pub fn shape(&self) -> Result<Shape, grana_geometry::Error> {
        Ok(match self {
            Self::Spots(set) => Shape::spots(Arc::from(set.spots())),
            Self::Surfaces(set) => Shape::mesh(set.geometry()?),
            Self::Filaments(set) => Shape::mesh(set.geometry()?),
        })
    }

    pub fn geometry_bytes(&self) -> u64 {
        match self {
            Self::Spots(_) => 0,
            Self::Surfaces(set) => set.bytes(),
            Self::Filaments(set) => set.bytes(),
        }
    }

    pub fn intensities(&self) -> &IntensityTable {
        match self {
            Self::Spots(set) => set.intensities(),
            Self::Surfaces(set) => set.intensities(),
            Self::Filaments(set) => set.intensities(),
        }
    }

    pub fn objects(&self) -> Objects<'_> {
        match self {
            Self::Spots(set) => Objects::Spots(set),
            Self::Surfaces(set) => Objects::Surfaces(set),
            Self::Filaments(set) => Objects::Filaments(set),
        }
    }

    pub fn centroids(&self) -> Vec<[f32; 3]> {
        match self {
            Self::Spots(set) => set.spots().iter().map(|spot| spot.center()).collect(),
            Self::Surfaces(set) => set
                .objects()
                .iter()
                .map(|object| object.centroid())
                .collect(),
            Self::Filaments(set) => set
                .objects()
                .iter()
                .map(|object| object.centroid())
                .collect(),
        }
    }

    pub fn spots_of(&self) -> &[Spot] {
        match self {
            Self::Spots(set) => set.spots(),
            Self::Surfaces(_) | Self::Filaments(_) => &[],
        }
    }

    pub fn without(&self, object: usize) -> Result<Option<Self>, grana_geometry::Error> {
        if object >= self.object_count() {
            return Ok(None);
        }
        Ok(match self {
            Self::Spots(set) => set.without(object).map(Self::Spots),
            Self::Surfaces(set) => set.without(object)?.map(Self::Surfaces),
            Self::Filaments(set) => set.without(object)?.map(Self::Filaments),
        })
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Option<Self>, grana_geometry::Error> {
        assert_eq!(
            keep.len(),
            self.object_count(),
            "the kept objects cover every object of the frame"
        );
        let kept = keep.iter().filter(|kept| **kept).count();
        if kept == 0 {
            return Ok(None);
        }
        if kept == keep.len() {
            return Ok(Some(self.clone()));
        }
        Ok(Some(match self {
            Self::Spots(set) => Self::Spots(set.retain(keep)),
            Self::Surfaces(set) => Self::Surfaces(set.retain(keep)?),
            Self::Filaments(set) => Self::Filaments(set.retain(keep)?),
        }))
    }
}
