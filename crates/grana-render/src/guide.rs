use wgpu::{
    Device, Extent3d, Texture, TextureDescriptor, TextureDimension, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::target::NORMAL_FORMAT;

pub(crate) struct Guide {
    view: TextureView,
    _texture: Texture,
}

impl Guide {
    pub(crate) fn new(device: &Device, size: [u32; 2]) -> Self {
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
        Self {
            view,
            _texture: texture,
        }
    }

    pub(crate) fn view(&self) -> &TextureView {
        &self.view
    }
}
