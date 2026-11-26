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
    let extent = extent(size);
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: extent[0],
            height: extent[1],
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

pub(crate) fn bytes(format: TextureFormat, size: [u32; 2]) -> u64 {
    let extent = extent(size);
    let pixel = u64::from(
        format
            .block_copy_size(None)
            .expect("an export colour target holds copyable texels"),
    );
    u64::from(extent[0]) * u64::from(extent[1]) * pixel
}

fn extent(size: [u32; 2]) -> [u32; 2] {
    [size[0].max(1), size[1].max(1)]
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
