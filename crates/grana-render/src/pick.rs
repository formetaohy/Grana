use wgpu::{
    Device, Extent3d, Texture, TextureDescriptor, TextureDimension, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::target::DEPTH_FORMAT;
use crate::target::PICK_FORMAT;

pub(crate) const PICK_ROW: u64 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickId {
    None,
    Style(u32),
    Marker(u32),
}

pub(crate) struct PickTarget {
    color: Texture,
    view: TextureView,
    depth: TextureView,
}

impl PickTarget {
    fn new(device: &Device) -> Self {
        let extent = Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&TextureDescriptor {
            label: Some("grana.pick.color"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: PICK_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&TextureViewDescriptor::default());
        let depth = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.pick.depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        Self { color, view, depth }
    }

    pub(crate) fn ensure<'a>(device: &Device, slot: &'a mut Option<PickTarget>) -> &'a PickTarget {
        slot.get_or_insert_with(|| Self::new(device))
    }

    pub(crate) fn color(&self) -> &Texture {
        &self.color
    }

    pub(crate) fn view(&self) -> &TextureView {
        &self.view
    }

    pub(crate) fn depth(&self) -> &TextureView {
        &self.depth
    }
}

pub(crate) fn decode(bytes: &[u8]) -> PickId {
    let (words, _) = bytes.as_chunks::<4>();
    if words.len() < 2 {
        return PickId::None;
    }
    let kind = u32::from_ne_bytes(words[0]);
    let value = u32::from_ne_bytes(words[1]);
    match (kind, value) {
        (1, 1..) => PickId::Style(value - 1),
        (2, 1..) => PickId::Marker(value - 1),
        _ => PickId::None,
    }
}
