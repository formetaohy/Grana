use wgpu::{
    CommandEncoderDescriptor, Device, Extent3d, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::target::NORMAL_FORMAT;

pub(crate) struct Guide {
    view: TextureView,
    _texture: Texture,
}

impl Guide {
    pub(crate) fn new(device: &Device, queue: &Queue, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a guide target covers a positive surface"
        );
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.guide"),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: NORMAL_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("grana.guide.view"),
            ..Default::default()
        });
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grana.guide"),
        });
        clear(&mut encoder, &view);
        queue.submit([encoder.finish()]);
        Self {
            view,
            _texture: texture,
        }
    }

    pub(crate) fn view(&self) -> &TextureView {
        &self.view
    }

    pub(crate) fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        clear(encoder, &self.view);
    }
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.guide.clear.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        ..Default::default()
    });
}
