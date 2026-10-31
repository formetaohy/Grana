use grana_analysis::{FilamentParams, SpotParams, SurfaceParams, TrackParams};
use grana_measure::{Measurement, Objects};
use grana_object::{FilamentSet, IntensityTable, PackedMesh, Spot, SpotSet, SurfaceSet};

const SPOT_STEP: f32 = 8.0;
const SURFACE_STEP: f32 = 12.0;
const SPOT_THICKNESS: f32 = 1.0;
const SURFACE_THICKNESS: f32 = 2.0;
const MAX_GAP: u32 = 1;
const DIVISION_SCALE: f32 = 2.0;
const SPOT_DIAMETER: f32 = 4.0;
const SPOT_CONTRAST: f32 = 0.2;
const SURFACE_THRESHOLD: f32 = 0.5;
const SURFACE_MIN_VOLUME: f32 = 0.0;
const FILAMENT_THRESHOLD: f32 = 0.5;
const FILAMENT_MIN_LENGTH: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Spots,
    Surfaces,
    Filaments,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamBound {
    Voxel(f32),
    Unit,
    Volume,
    Length(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamField {
    pub label: &'static str,
    pub suffix: &'static str,
    pub bound: ParamBound,
    pub logarithmic: bool,
}

const SPOT_FIELDS: [ParamField; 2] = [
    ParamField {
        label: "diameter",
        suffix: " µm",
        bound: ParamBound::Voxel(16.0),
        logarithmic: true,
    },
    ParamField {
        label: "contrast",
        suffix: "",
        bound: ParamBound::Unit,
        logarithmic: false,
    },
];

const SURFACE_FIELDS: [ParamField; 2] = [
    ParamField {
        label: "threshold",
        suffix: "",
        bound: ParamBound::Unit,
        logarithmic: false,
    },
    ParamField {
        label: "min volume",
        suffix: " µm³",
        bound: ParamBound::Volume,
        logarithmic: false,
    },
];

const FILAMENT_FIELDS: [ParamField; 2] = [
    ParamField {
        label: "threshold",
        suffix: "",
        bound: ParamBound::Unit,
        logarithmic: false,
    },
    ParamField {
        label: "min length",
        suffix: " µm",
        bound: ParamBound::Length(0.25),
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
            Self::Surfaces => {
                ObjectParams::Surfaces(SurfaceParams::new(SURFACE_THRESHOLD, SURFACE_MIN_VOLUME))
            }
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
}

impl TrackStyle {
    pub fn new(params: TrackParams, thickness: f32) -> Self {
        assert!(
            thickness.is_finite() && thickness > 0.0,
            "a track tube is a positive thickness"
        );
        Self { params, thickness }
    }

    pub fn params(self) -> TrackParams {
        self.params
    }

    pub fn thickness(self) -> f32 {
        self.thickness
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
        assert!(index < 2, "an object kind carries two parameters");
        match (self, index) {
            (Self::Spots(params), 0) => params.diameter(),
            (Self::Spots(params), _) => params.threshold(),
            (Self::Surfaces(params), 0) => params.threshold(),
            (Self::Surfaces(params), _) => params.min_volume(),
            (Self::Filaments(params), 0) => params.threshold(),
            (Self::Filaments(params), _) => params.min_length(),
        }
    }

    pub fn set_value(&mut self, index: usize, value: f32) {
        assert!(index < 2, "an object kind carries two parameters");
        match (self, index) {
            (Self::Spots(params), 0) => params.set_diameter(value),
            (Self::Spots(params), _) => params.set_threshold(value),
            (Self::Surfaces(params), 0) => params.set_threshold(value),
            (Self::Surfaces(params), _) => params.set_min_volume(value),
            (Self::Filaments(params), 0) => params.set_threshold(value),
            (Self::Filaments(params), _) => params.set_min_length(value),
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

    pub fn geometry(&self) -> Option<&PackedMesh> {
        match self {
            Self::Spots(_) => None,
            Self::Surfaces(set) => Some(set.geometry()),
            Self::Filaments(set) => Some(set.geometry()),
        }
    }

    pub fn geometry_bytes(&self) -> u64 {
        self.geometry().map_or(0, PackedMesh::bytes)
    }

    pub fn intensities(&self) -> &IntensityTable {
        match self {
            Self::Spots(set) => set.intensities(),
            Self::Surfaces(set) => set.intensities(),
            Self::Filaments(set) => set.intensities(),
        }
    }

    pub(crate) fn objects(&self) -> Objects<'_> {
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
}
