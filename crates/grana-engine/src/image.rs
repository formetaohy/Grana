use wgpu::{
    Device, Extent3d, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor,
};

pub(crate) struct ViewportImage {
    pub(crate) size: [u32; 2],
    pub(crate) view: TextureView,
    _texture: Texture,
}

impl ViewportImage {
    pub(crate) fn new(device: &Device, size: [u32; 2], format: TextureFormat) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.viewport"),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        Self {
            size,
            view,
            _texture: texture,
        }
    }
}
