use grana_display::Camera;
use grana_render::{Display, Frame, FrameInfo, Renderer, Temporal};
use grana_scene::Scene;
use wgpu::{Device, Queue, TextureView};

use crate::view;

const HISTORY: f32 = 8.0;

pub(crate) struct View<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub viewport: [u32; 4],
    pub size: [u32; 2],
    pub samples: u32,
    pub display: Display,
    pub level_cap: usize,
}

pub(crate) fn draw(
    renderer: &mut Renderer,
    device: &Device,
    queue: &Queue,
    frame: View<'_>,
    rendered: &view::RenderedScene,
) -> FrameInfo {
    renderer.set_transparent(false);
    let scene = rendered.frame();
    renderer.draw(
        device,
        queue,
        Frame {
            target: frame.target,
            size: frame.size,
            camera: frame.camera,
            viewport: frame.viewport,
            time: frame.scene.time(),
            region: frame.scene.region,
            clip: frame.scene.clip,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &scene,
            display: frame.display,
            level_cap: frame.level_cap,
            temporal: Some(Temporal::new(HISTORY)),
        },
    )
}
