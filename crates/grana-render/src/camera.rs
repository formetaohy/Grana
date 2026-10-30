use glam::{Mat4, Vec2, Vec3};

const PITCH_LIMIT: f32 = 1.45;
const ORBIT_SENSITIVITY: f32 = 0.007;
const ZOOM_SENSITIVITY: f32 = 0.15;

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

    pub fn light_direction(&self) -> Vec3 {
        (-self.right() * 0.45 + self.up() * 0.55 - self.forward() * 0.7).normalize()
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
}
