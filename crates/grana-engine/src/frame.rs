use grana_display::Camera;
use grana_render::{Delivery, Frame, FrameInfo, Renderer};
use grana_scene::Scene;
use wgpu::{Device, Queue, TextureView};

use crate::view;

pub struct View<'a> {
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub delivery: Delivery,
    pub overlays: &'a [grana_render::Overlay<'a>],
}

pub(crate) struct Draw<'a> {
    pub target: &'a TextureView,
    pub view: View<'a>,
}

pub(crate) fn draw(
    renderer: &mut Renderer,
    device: &Device,
    queue: &Queue,
    stage: Draw<'_>,
    rendered: &view::RenderedScene,
) -> Result<FrameInfo, String> {
    let Draw { target, view } = stage;
    renderer.set_transparent(false);
    let scene = rendered.frame(view.overlays);
    renderer.draw(
        device,
        queue,
        Frame {
            target,
            size: view.size,
            camera: view.camera,
            viewport: [0, 0, view.size[0], view.size[1]],
            time: view.scene.time(),
            region: view.scene.region,
            clip: view.scene.clip,
            exposure: view.scene.exposure,
            scene: &scene,
            delivery: view.delivery,
        },
    )
}
