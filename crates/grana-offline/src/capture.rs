use wgpu::{
    CommandEncoder, Device, Extent3d, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView,
};

pub(crate) fn texture(
    device: &Device,
    label: &'static str,
    format: TextureFormat,
    size: [u32; 2],
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

pub(crate) fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.export.overlay.clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}
