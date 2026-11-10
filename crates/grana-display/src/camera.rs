use glam::{Mat4, Vec2, Vec3, Vec4};

const PITCH_LIMIT: f32 = 1.45;
const ORBIT_SENSITIVITY: f32 = 0.007;
const ZOOM_SENSITIVITY: f32 = 0.15;
const GOLDEN_ANGLE: f32 = 2.399_963_2;
pub const APERTURE_LIMIT: f32 = 1.0e4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraProjection {
    #[default]
    Perspective,
    Orthographic,
}

impl CameraProjection {
    pub const ALL: [Self; 2] = [Self::Perspective, Self::Orthographic];

    pub fn name(self) -> &'static str {
        match self {
            Self::Perspective => "perspective",
            Self::Orthographic => "orthographic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Perspective => "Perspective",
            Self::Orthographic => "Orthographic",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Perspective => 0,
            Self::Orthographic => 1,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    pub fn orthographic(self) -> bool {
        matches!(self, Self::Orthographic)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    pub pivot: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub projection: CameraProjection,
    pub aperture: f32,
    pub focus: f32,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pivot: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    fov_y: f32,
    projection: CameraProjection,
    aperture: f32,
    focus: f32,
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
            projection: CameraProjection::Perspective,
            aperture: 0.0,
            focus: 1.0,
        }
    }

    pub fn fov_y(&self) -> f32 {
        self.fov_y
    }

    pub fn pivot(&self) -> Vec3 {
        self.pivot
    }

    pub fn distance(&self) -> f32 {
        self.distance
    }

    pub fn projection(&self) -> CameraProjection {
        self.projection
    }

    pub fn set_projection(&mut self, projection: CameraProjection) {
        self.projection = projection;
    }

    pub fn aperture(&self) -> f32 {
        self.aperture
    }

    pub fn set_aperture(&mut self, aperture: f32) {
        assert!(
            aperture.is_finite() && (0.0..=APERTURE_LIMIT).contains(&aperture),
            "an aperture is a bounded length"
        );
        self.aperture = aperture;
    }

    pub fn focus(&self) -> f32 {
        self.focus
    }

    pub fn set_focus(&mut self, focus: f32) {
        assert!(
            focus.is_finite() && focus > 0.0 && focus <= APERTURE_LIMIT,
            "a focus distance is a bounded positive length"
        );
        self.focus = focus;
    }

    pub fn focused(&self) -> bool {
        self.aperture > 0.0 && !self.projection.orthographic()
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

    pub fn half_height(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    pub fn pixels_per_world(&self, height: u32) -> f32 {
        self.pixels_per_world_at(height.max(1) as f32)
    }

    pub fn pixels_per_world_at(&self, height: f32) -> f32 {
        let height = height.max(1.0);
        match self.projection {
            CameraProjection::Perspective => 0.5 * height / (0.5 * self.fov_y).tan(),
            CameraProjection::Orthographic => height / (2.0 * self.half_height().max(1.0e-6)),
        }
    }

    pub fn world_per_point(&self, height: f32, distance: f32) -> f32 {
        let reach = match self.projection {
            CameraProjection::Perspective => distance.abs(),
            CameraProjection::Orthographic => 1.0,
        };
        reach.max(1.0e-6) / self.pixels_per_world_at(height)
    }

    pub fn view_projection(&self, aspect: f32, scene_radius: f32) -> Mat4 {
        assert!(aspect > 0.0, "aspect ratio must be positive");
        assert!(scene_radius > 0.0, "scene radius must be positive");
        let far = self.distance + scene_radius * 4.0;
        let projection = match self.projection {
            CameraProjection::Perspective => {
                let near = (self.distance - scene_radius)
                    .max(scene_radius * 0.01)
                    .max(1e-4);
                Mat4::perspective_rh(self.fov_y, aspect, near, far)
            }
            CameraProjection::Orthographic => {
                let half = self.half_height();
                Mat4::orthographic_rh(-half * aspect, half * aspect, -half, half, 0.0, far)
            }
        };
        projection * Mat4::look_at_rh(self.eye(), self.pivot, Vec3::Y)
    }

    pub fn lens(&self, index: u32, samples: u32) -> Vec2 {
        if !self.focused() {
            return Vec2::ZERO;
        }
        let samples = samples.max(1);
        let radius = self.aperture * ((index as f32 + 0.5) / samples as f32).sqrt();
        let angle = index as f32 * GOLDEN_ANGLE;
        Vec2::new(radius * angle.cos(), radius * angle.sin())
    }

    pub fn ray(&self, ndc: Vec2, aspect: f32) -> Ray {
        assert!(aspect > 0.0, "aspect ratio must be positive");
        let eye = self.eye();
        match self.projection {
            CameraProjection::Perspective => {
                let projection = self.view_projection(aspect, self.distance.max(1.0e-4));
                let clipped = projection.inverse() * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
                Ray {
                    origin: eye,
                    direction: (clipped.truncate() / clipped.w - eye).normalize(),
                }
            }
            CameraProjection::Orthographic => {
                let half = self.half_height();
                Ray {
                    origin: eye
                        + self.right() * (ndc.x * half * aspect)
                        + self.up() * (ndc.y * half),
                    direction: self.forward(),
                }
            }
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
        self.focus = self.distance;
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SENSITIVITY;
        self.pitch = (self.pitch + delta.y * ORBIT_SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let scale = 2.0 * self.half_height() / viewport_height.max(1.0);
        self.pivot += (self.right() * -delta.x + self.up() * delta.y) * scale;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (1.0 - scroll * ZOOM_SENSITIVITY)).clamp(1e-4, 1.0e7);
    }

    pub fn focus_at_pivot(&mut self) {
        self.focus = self.distance;
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
            projection: self.projection,
            aperture: self.aperture,
            focus: self.focus,
        }
    }

    pub fn restore(&mut self, state: CameraState) {
        let CameraState {
            pivot,
            yaw,
            pitch,
            distance,
            fov_y,
            projection,
            aperture,
            focus,
        } = state;
        assert!(
            pivot.iter().all(|value| value.is_finite())
                && yaw.is_finite()
                && pitch.is_finite()
                && distance.is_finite()
                && distance > 0.0
                && fov_y > 1.0f32.to_radians()
                && fov_y < 179.0f32.to_radians()
                && aperture.is_finite()
                && (0.0..=APERTURE_LIMIT).contains(&aperture)
                && focus.is_finite()
                && focus > 0.0
                && focus <= APERTURE_LIMIT,
            "a camera state describes a plausible view"
        );
        self.pivot = Vec3::from(pivot);
        self.yaw = yaw;
        self.pitch = pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.distance = distance;
        self.fov_y = fov_y;
        self.projection = projection;
        self.aperture = aperture;
        self.focus = focus;
    }
}
