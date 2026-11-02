use grana_display::{Camera, CameraState, SectionCamera, SectionState};
use grana_render::{Frame, FrameInfo, Renderer};
use grana_scene::Scene;
use wgpu::{Device, Queue, TextureView};

use crate::view;

pub(crate) struct View<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub samples: u32,
    pub prefetch: bool,
    pub transparent: bool,
}

pub(crate) fn draw(
    renderer: &mut Renderer,
    device: &Device,
    queue: &Queue,
    frame: View<'_>,
) -> FrameInfo {
    let rendered = view::frame(frame.scene);
    renderer.set_transparent(frame.transparent);
    renderer.draw(
        device,
        queue,
        Frame {
            target: frame.target,
            size: frame.size,
            volume_region: frame.volume_region,
            section_region: frame.section_region,
            time: frame.scene.time(),
            camera: frame.camera,
            section: frame.section,
            region: frame.scene.region,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &rendered.frame(),
            prefetch: frame.prefetch,
        },
    )
}

pub(crate) fn restored_camera(state: &CameraState) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.restore(*state);
    camera
}

pub(crate) fn restored_section(state: &SectionState) -> SectionCamera {
    let mut section = SectionCamera::new(state.axis);
    section.restore(*state);
    section
}
