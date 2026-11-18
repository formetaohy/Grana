use wgpu::{
    CommandEncoder, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp, TextureView,
};

pub(crate) fn scene(encoder: &mut CommandEncoder, color: &TextureView, depth: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.scene.clear.pass"),
        color_attachments: &[Some(cleared(color, wgpu::Color::TRANSPARENT))],
        depth_stencil_attachment: Some(depth_cleared(depth, 1.0)),
        ..Default::default()
    });
}

pub(crate) fn volume(
    encoder: &mut CommandEncoder,
    radiance: &TextureView,
    surface: &TextureView,
    normal: &TextureView,
) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.volume.clear.pass"),
        color_attachments: &[
            Some(cleared(radiance, wgpu::Color::TRANSPARENT)),
            Some(cleared(
                surface,
                wgpu::Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            )),
            Some(cleared(normal, wgpu::Color::TRANSPARENT)),
        ],
        depth_stencil_attachment: None,
        ..Default::default()
    });
}

fn cleared(view: &TextureView, color: wgpu::Color) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(color),
            store: StoreOp::Store,
        },
    }
}

fn depth_cleared(view: &TextureView, depth: f32) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(Operations {
            load: LoadOp::Clear(depth),
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    }
}
