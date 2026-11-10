use grana_display::{ChannelMask, PANE_COUNT, Quality};
use grana_render::{Frame, FrameInfo, PaneFrame, Renderer};
use grana_scene::Scene;
use wgpu::{Device, Queue, TextureView};

use crate::view;

pub(crate) struct View<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub panes: [Option<PaneFrame<'a>>; PANE_COUNT],
    pub channels: [ChannelMask; PANE_COUNT],
    pub quality: [Quality; PANE_COUNT],
    pub size: [u32; 2],
    pub samples: u32,
    pub prefetch: bool,
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
            panes: frame.panes,
            time: frame.scene.time(),
            region: frame.scene.region,
            clip: frame.scene.clip,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &scene,
            pane_channels: frame.channels,
            pane_quality: frame.quality,
            prefetch: frame.prefetch,
            level_cap: frame.level_cap,
            assured: false,
            temporal: None,
        },
    )
}
