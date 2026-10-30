use grana_analysis::TrackParams;
use grana_object::{Mesh, TrackSet, TrackSource};
use grana_render::track_mesh;

const SPOT_STEP: f32 = 8.0;
const SURFACE_STEP: f32 = 12.0;
const SPOT_THICKNESS: f32 = 1.0;
const SURFACE_THICKNESS: f32 = 2.0;
const MIN_THICKNESS: f32 = 0.25;
const MAX_THICKNESS: f32 = 40.0;
const GAP: u32 = 1;
const DIVISION_SCALE: f32 = 2.0;

pub struct Layer {
    set: TrackSet,
    mesh: Mesh,
    visible: bool,
    thickness: f32,
}

impl Layer {
    pub fn set(&self) -> &TrackSet {
        &self.set
    }

    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn thickness(&self) -> f32 {
        self.thickness
    }
}

pub struct Tracks {
    spot: Option<Layer>,
    surface: Option<Layer>,
    spot_params: TrackParams,
    surface_params: TrackParams,
    voxel: f32,
    revision: u64,
}

impl Tracks {
    pub fn new(voxel: f32) -> Self {
        assert!(
            voxel.is_finite() && voxel > 0.0,
            "the voxel size is a positive length"
        );
        let spot_step = SPOT_STEP * voxel;
        let surface_step = SURFACE_STEP * voxel;
        Self {
            spot: None,
            surface: None,
            spot_params: TrackParams::new(spot_step, GAP, true, DIVISION_SCALE * spot_step),
            surface_params: TrackParams::new(
                surface_step,
                GAP,
                true,
                DIVISION_SCALE * surface_step,
            ),
            voxel,
            revision: 1,
        }
    }

    pub fn layer(&self, source: TrackSource) -> Option<&Layer> {
        match source {
            TrackSource::Spots => self.spot.as_ref(),
            TrackSource::Surfaces => self.surface.as_ref(),
        }
    }

    pub fn params(&self, source: TrackSource) -> TrackParams {
        match source {
            TrackSource::Spots => self.spot_params,
            TrackSource::Surfaces => self.surface_params,
        }
    }

    pub fn set_params(&mut self, source: TrackSource, params: TrackParams) {
        match source {
            TrackSource::Spots => self.spot_params = params,
            TrackSource::Surfaces => self.surface_params = params,
        }
    }

    pub fn voxel(&self) -> f32 {
        self.voxel
    }

    pub fn min_thickness(&self) -> f32 {
        MIN_THICKNESS * self.voxel
    }

    pub fn max_thickness(&self) -> f32 {
        MAX_THICKNESS * self.voxel
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn install(&mut self, set: TrackSet) {
        let source = set.source();
        let previous = self.layer(source);
        let visible = previous.is_none_or(Layer::visible);
        let thickness = previous.map_or(self.default_thickness(source), Layer::thickness);
        let mesh = track_mesh(&set, thickness);
        let layer = Layer {
            set,
            mesh,
            visible,
            thickness,
        };
        match source {
            TrackSource::Spots => self.spot = Some(layer),
            TrackSource::Surfaces => self.surface = Some(layer),
        }
        self.revision += 1;
    }

    pub fn clear(&mut self, source: TrackSource) {
        let slot = match source {
            TrackSource::Spots => &mut self.spot,
            TrackSource::Surfaces => &mut self.surface,
        };
        if slot.take().is_some() {
            self.revision += 1;
        }
    }

    pub fn set_visible(&mut self, source: TrackSource, visible: bool) {
        if let Some(layer) = self.layer_mut(source) {
            layer.visible = visible;
        }
    }

    pub fn set_thickness(&mut self, source: TrackSource, thickness: f32) {
        let thickness = thickness.clamp(self.min_thickness(), self.max_thickness());
        let mut changed = false;
        if let Some(layer) = self.layer_mut(source)
            && (layer.thickness - thickness).abs() > f32::EPSILON
        {
            layer.thickness = thickness;
            layer.mesh = track_mesh(&layer.set, thickness);
            changed = true;
        }
        if changed {
            self.revision += 1;
        }
    }

    fn layer_mut(&mut self, source: TrackSource) -> Option<&mut Layer> {
        match source {
            TrackSource::Spots => self.spot.as_mut(),
            TrackSource::Surfaces => self.surface.as_mut(),
        }
    }

    fn default_thickness(&self, source: TrackSource) -> f32 {
        let scale = match source {
            TrackSource::Spots => SPOT_THICKNESS,
            TrackSource::Surfaces => SURFACE_THICKNESS,
        };
        scale * self.voxel
    }
}
