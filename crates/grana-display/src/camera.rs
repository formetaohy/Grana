use glam::{Mat4, Vec2, Vec3, Vec4};

const PITCH_LIMIT: f32 = 1.45;
const ORBIT_SENSITIVITY: f32 = 0.007;
const ZOOM_SENSITIVITY: f32 = 0.15;
const SECTION_ZOOM_SENSITIVITY: f32 = 0.15;
const FIT_MARGIN: f32 = 1.05;
const MIN_SPAN_VOXELS: f32 = 8.0;
const ELEVATION_LIMIT: f32 = std::f32::consts::FRAC_PI_2;
pub const THICKNESS_LIMIT: f32 = 1.0e4;
pub const PROJECTION_THICKNESS: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    pub pivot: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    #[default]
    Plane,
    Maximum,
    Average,
}

impl Projection {
    pub const ALL: [Self; 3] = [Self::Plane, Self::Maximum, Self::Average];

    pub fn name(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Maximum => "maximum",
            Self::Average => "average",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plane => "Plane",
            Self::Maximum => "Max",
            Self::Average => "Mean",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Plane => 0,
            Self::Maximum => 1,
            Self::Average => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    pub fn projected(self) -> bool {
        !matches!(self, Self::Plane)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionState {
    pub center: [f32; 3],
    pub azimuth: f32,
    pub elevation: f32,
    pub span: f32,
    pub projection: Projection,
    pub thickness: f32,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pivot: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    fov_y: f32,
}

impl Camera {
    pub fn new(fov_y_degrees: f32) -> Self {
        assert!(
            fov_y_degrees > 1.0 && fov_y_degrees < 179.0,
            "field of view must be a plausible angle"
        );
        Self {
            pivot: Vec3::ZERO,
            yaw: 0.55,
            pitch: 0.32,
            distance: 1.0,
            fov_y: fov_y_degrees.to_radians(),
        }
    }

    pub fn fov_y(&self) -> f32 {
        self.fov_y
    }

    pub fn distance(&self) -> f32 {
        self.distance
    }

    pub fn eye(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        self.pivot + Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw) * self.distance
    }

    pub fn forward(&self) -> Vec3 {
        (self.pivot - self.eye()).normalize()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward())
    }

    pub fn view_projection(&self, aspect: f32, scene_radius: f32) -> Mat4 {
        assert!(aspect > 0.0, "aspect ratio must be positive");
        assert!(scene_radius > 0.0, "scene radius must be positive");
        let near = (self.distance - scene_radius)
            .max(scene_radius * 0.01)
            .max(1e-4);
        let far = self.distance + scene_radius * 4.0;
        let projection = Mat4::perspective_rh(self.fov_y, aspect, near, far);
        projection * Mat4::look_at_rh(self.eye(), self.pivot, Vec3::Y)
    }

    pub fn ray(&self, ndc: Vec2, aspect: f32) -> Ray {
        assert!(aspect > 0.0, "aspect ratio must be positive");
        let projection = self.view_projection(aspect, self.distance.max(1.0e-4));
        let clipped = projection.inverse() * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let origin = self.eye();
        Ray {
            origin,
            direction: (clipped.truncate() / clipped.w - origin).normalize(),
        }
    }

    pub fn fit(&mut self, world_size: Vec3, aspect: f32) {
        assert!(world_size.length() > 0.0, "world size must be positive");
        assert!(aspect > 0.0, "aspect ratio must be positive");
        let half = 0.5 * world_size;
        let tan_vertical = (self.fov_y * 0.5).tan();
        let tan_horizontal = tan_vertical * aspect;
        let right = self.right();
        let up = self.up();
        let forward = self.forward();
        let mut distance: f32 = 0.0;
        for index in 0..8u32 {
            let corner = Vec3::new(
                if index & 1 == 0 { -half.x } else { half.x },
                if index & 2 == 0 { -half.y } else { half.y },
                if index & 4 == 0 { -half.z } else { half.z },
            );
            let depth = corner.dot(forward);
            let horizontal = corner.dot(right).abs() / tan_horizontal;
            let vertical = corner.dot(up).abs() / tan_vertical;
            distance = distance.max(horizontal.max(vertical) - depth);
        }
        self.pivot = Vec3::ZERO;
        self.distance = distance * 1.06;
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SENSITIVITY;
        self.pitch = (self.pitch + delta.y * ORBIT_SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let scale = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        self.pivot += (self.right() * -delta.x + self.up() * delta.y) * scale;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (1.0 - scroll * ZOOM_SENSITIVITY)).clamp(1e-4, 1.0e7);
    }

    pub fn focus(&self) -> Vec3 {
        self.pivot
    }

    pub fn clamp_focus(&mut self, half: Vec3) {
        self.pivot = self.pivot.clamp(-half, half);
    }

    pub fn state(&self) -> CameraState {
        CameraState {
            pivot: self.pivot.to_array(),
            yaw: self.yaw,
            pitch: self.pitch,
            distance: self.distance,
            fov_y: self.fov_y,
        }
    }

    pub fn restore(&mut self, state: CameraState) {
        let CameraState {
            pivot,
            yaw,
            pitch,
            distance,
            fov_y,
        } = state;
        assert!(
            pivot.iter().all(|value| value.is_finite())
                && yaw.is_finite()
                && pitch.is_finite()
                && distance.is_finite()
                && distance > 0.0
                && fov_y > 1.0f32.to_radians()
                && fov_y < 179.0f32.to_radians(),
            "a camera state describes a plausible view"
        );
        self.pivot = Vec3::from(pivot);
        self.yaw = yaw;
        self.pitch = pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.distance = distance;
        self.fov_y = fov_y;
    }
}

#[derive(Clone, Debug)]
pub struct SectionCamera {
    center: Vec3,
    azimuth: f32,
    elevation: f32,
    span: f32,
    min_span: f32,
    max_span: f32,
    projection: Projection,
    thickness: f32,
}

impl Default for SectionCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl SectionCamera {
    pub fn new() -> Self {
        Self {
            center: Vec3::ZERO,
            azimuth: 0.0,
            elevation: 0.0,
            span: 1.0,
            min_span: 1.0e-4,
            max_span: 1.0e4,
            projection: Projection::Plane,
            thickness: PROJECTION_THICKNESS,
        }
    }

    pub fn center(&self) -> Vec3 {
        self.center
    }

    pub fn azimuth(&self) -> f32 {
        self.azimuth
    }

    pub fn elevation(&self) -> f32 {
        self.elevation
    }

    pub fn span(&self) -> f32 {
        self.span
    }

    pub fn projection(&self) -> Projection {
        self.projection
    }

    pub fn thickness(&self) -> f32 {
        self.thickness
    }

    pub fn projected(&self) -> bool {
        self.projection.projected() && self.thickness > 0.0
    }

    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
    }

    pub fn set_thickness(&mut self, thickness: f32) {
        assert!(
            thickness.is_finite() && (0.0..=THICKNESS_LIMIT).contains(&thickness),
            "a projection thickness is a bounded length"
        );
        self.thickness = thickness;
    }

    pub fn normal(&self) -> Vec3 {
        let (sin_azimuth, cos_azimuth) = self.azimuth.sin_cos();
        let (sin_elevation, cos_elevation) = self.elevation.sin_cos();
        Vec3::new(
            cos_elevation * sin_azimuth,
            sin_elevation,
            cos_elevation * cos_azimuth,
        )
    }

    pub fn right(&self) -> Vec3 {
        let (sin_azimuth, cos_azimuth) = self.azimuth.sin_cos();
        Vec3::new(cos_azimuth, 0.0, -sin_azimuth)
    }

    pub fn up(&self) -> Vec3 {
        self.normal().cross(self.right())
    }

    pub fn offset(&self) -> f32 {
        self.center.dot(self.normal())
    }

    pub fn set_orientation(&mut self, azimuth: f32, elevation: f32) {
        assert!(
            azimuth.is_finite() && elevation.is_finite(),
            "a section orientation is a finite pair of angles"
        );
        self.azimuth = wrap(azimuth);
        self.elevation = elevation.clamp(-ELEVATION_LIMIT, ELEVATION_LIMIT);
    }

    pub fn fit(&mut self, world: Vec3, voxel: Vec3, aspect: f32) {
        assert!(
            world.x > 0.0 && world.y > 0.0 && world.z > 0.0,
            "the volume world size must be positive"
        );
        assert!(aspect > 0.0, "aspect ratio must be positive");
        let width = world.abs().dot(self.right().abs());
        let height = world.abs().dot(self.up().abs());
        let span = height.max(width / aspect) * FIT_MARGIN;
        self.min_span = MIN_SPAN_VOXELS * voxel.min_element();
        self.max_span = (4.0 * world.length()).max(self.min_span);
        self.span = span.clamp(self.min_span, self.max_span);
        self.center = Vec3::ZERO;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.span = (self.span * (1.0 - scroll * SECTION_ZOOM_SENSITIVITY))
            .clamp(self.min_span, self.max_span);
    }

    pub fn pan(&mut self, delta: Vec2, world_per_point: f32) {
        self.center += (self.right() * -delta.x + self.up() * delta.y) * world_per_point;
    }

    pub fn shift(&mut self, distance: f32) {
        self.center += self.normal() * distance;
    }

    pub fn point(&self, ndc: Vec2, aspect: f32) -> Vec3 {
        assert!(aspect > 0.0, "aspect ratio must be positive");
        self.center
            + self.right() * (0.5 * ndc.x * self.span * aspect)
            + self.up() * (0.5 * ndc.y * self.span)
    }

    pub fn set_center(&mut self, center: Vec3) {
        assert!(
            center.is_finite(),
            "a section center is a finite world point"
        );
        self.center = center;
    }

    pub fn state(&self) -> SectionState {
        SectionState {
            center: self.center.to_array(),
            azimuth: self.azimuth,
            elevation: self.elevation,
            span: self.span,
            projection: self.projection,
            thickness: self.thickness,
        }
    }

    pub fn restore(&mut self, state: SectionState) {
        let SectionState {
            center,
            azimuth,
            elevation,
            span,
            projection,
            thickness,
        } = state;
        assert!(
            center.iter().all(|value| value.is_finite())
                && azimuth.is_finite()
                && elevation.is_finite()
                && span.is_finite()
                && span > 0.0
                && thickness.is_finite()
                && (0.0..=THICKNESS_LIMIT).contains(&thickness),
            "a section state describes a plausible plane"
        );
        self.center = Vec3::from(center);
        self.azimuth = wrap(azimuth);
        self.elevation = elevation.clamp(-ELEVATION_LIMIT, ELEVATION_LIMIT);
        self.span = span;
        self.projection = projection;
        self.thickness = thickness;
    }
}

fn wrap(angle: f32) -> f32 {
    let mut wrapped = angle % std::f32::consts::TAU;
    if wrapped > std::f32::consts::PI {
        wrapped -= std::f32::consts::TAU;
    } else if wrapped < -std::f32::consts::PI {
        wrapped += std::f32::consts::TAU;
    }
    wrapped
}
