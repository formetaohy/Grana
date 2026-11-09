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
    pub level_cap: usize,
    pub assured: bool,
    pub coarsen: usize,
}

pub(crate) fn draw(
    renderer: &mut Renderer,
    device: &Device,
    queue: &Queue,
    frame: View<'_>,
    rendered: &view::RenderedScene<'_>,
) -> FrameInfo {
    renderer.set_transparent(frame.transparent);
    let scene = rendered.frame();
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
            clip: frame.scene.clip,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &scene,
            prefetch: frame.prefetch,
            level_cap: frame.level_cap,
            assured: frame.assured,
            coarsen: frame.coarsen,
        },
    )
}

pub(crate) fn restored_camera(state: &CameraState) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.restore(*state);
    camera
}

pub(crate) fn restored_section(state: &SectionState) -> SectionCamera {
    let mut section = SectionCamera::new();
    section.restore(*state);
    section
}
