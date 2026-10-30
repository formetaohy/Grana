use glam::Vec3;
use grana_volume::Axis;

const ZOOM_SENSITIVITY: f32 = 0.15;
const FIT_MARGIN: f32 = 1.05;
const MIN_SPAN_VOXELS: f32 = 8.0;

#[derive(Clone, Debug)]
pub struct SectionCamera {
    axis: Axis,
    span: f32,
    min_span: f32,
    max_span: f32,
}

impl SectionCamera {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            span: 1.0,
            min_span: 1.0e-4,
            max_span: 1.0e4,
        }
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    pub fn span(&self) -> f32 {
        self.span
    }

    pub fn set_axis(&mut self, axis: Axis) {
        self.axis = axis;
    }

    pub fn fit(&mut self, world: Vec3, voxel: Vec3, aspect: f32) {
        assert!(
            world.x > 0.0 && world.y > 0.0 && world.z > 0.0,
            "the volume world size must be positive"
        );
        assert!(aspect > 0.0, "aspect ratio must be positive");
        let (u, v) = self.axis.in_plane();
        let world_u = world[u.index()];
        let world_v = world[v.index()];
        let span = world_v.max(world_u / aspect) * FIT_MARGIN;
        self.min_span = MIN_SPAN_VOXELS * voxel[u.index()].min(voxel[v.index()]);
        self.max_span = (4.0 * world.length()).max(self.min_span);
        self.span = span.clamp(self.min_span, self.max_span);
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.span =
            (self.span * (1.0 - scroll * ZOOM_SENSITIVITY)).clamp(self.min_span, self.max_span);
    }
}
