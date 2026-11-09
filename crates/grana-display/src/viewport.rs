use crate::{Camera, CameraState, SectionCamera, SectionState};

pub const SPLIT_MIN: f32 = 0.25;
pub const SPLIT_MAX: f32 = 0.8;

pub struct Viewport {
    camera: Camera,
    section: SectionCamera,
    split: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            camera: Camera::new(35.0),
            section: SectionCamera::new(),
            split: 0.62,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn section(&self) -> &SectionCamera {
        &self.section
    }

    pub fn section_mut(&mut self) -> &mut SectionCamera {
        &mut self.section
    }

    pub fn split(&self) -> f32 {
        self.split
    }

    pub fn set_split(&mut self, split: f32) {
        assert!(split.is_finite(), "a viewport split is a finite fraction");
        self.split = split.clamp(SPLIT_MIN, SPLIT_MAX);
    }

    pub fn camera_state(&self) -> CameraState {
        self.camera.state()
    }

    pub fn section_state(&self) -> SectionState {
        self.section.state()
    }

    pub fn fit(
        &mut self,
        world: glam::Vec3,
        voxel: glam::Vec3,
        volume_aspect: f32,
        section_aspect: f32,
    ) {
        self.camera.fit(world, volume_aspect);
        self.section.fit(world, voxel, section_aspect);
    }

    pub fn fit_section(&mut self, world: glam::Vec3, voxel: glam::Vec3, aspect: f32) {
        self.section.fit(world, voxel, aspect);
    }

    pub fn clamp_focus(&mut self, half: glam::Vec3) {
        self.camera.clamp_focus(half);
    }

    pub fn restore(&mut self, camera: CameraState, section: SectionState) {
        self.camera.restore(camera);
        self.section.restore(section);
    }
}
