use grana_display::{Camera, CameraState, PANE_COUNT, SectionCamera, SectionState};
use grana_render::{Frame, FrameInfo, PaneFrame, Renderer};
use grana_scene::Scene;
use wgpu::{Device, Queue, TextureView};

use crate::view;

pub(crate) struct View<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub panes: [Option<PaneFrame<'a>>; PANE_COUNT],
    pub size: [u32; 2],
    pub samples: u32,
    pub prefetch: bool,
    pub transparent: bool,
    pub level_cap: usize,
    pub assured: bool,
}

pub(crate) fn draw(
    renderer: &mut Renderer,
    device: &Device,
    queue: &Queue,
    frame: View<'_>,
    rendered: &view::RenderedScene,
) -> FrameInfo {
    renderer.set_transparent(frame.transparent);
    let scene = rendered.frame();
    renderer.draw(
        device,
        queue,
        Frame {
            target: frame.target,
            size: frame.size,
            panes: frame.panes,
            time: frame.scene.time(),
            region: frame.scene.region,
            clip: frame.scene.clip,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &scene,
            prefetch: frame.prefetch,
            level_cap: frame.level_cap,
            assured: frame.assured,
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
