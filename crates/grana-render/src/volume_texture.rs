use grana_volume::Volume;
use half::f16;
use wgpu::{
    Device, Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension,
};

pub struct VolumeTexture {
    view: TextureView,
    level_count: u32,
}

impl VolumeTexture {
    pub fn new(device: &Device, queue: &Queue, volume: &Volume) -> Self {
        let extent = volume.extent();
        let level_count = volume.level_count();
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume"),
            size: Extent3d {
                width: extent.x(),
                height: extent.y(),
                depth_or_array_layers: extent.z(),
            },
            mip_level_count: level_count,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: TextureFormat::R16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut quantized = Vec::new();
        for (index, level) in volume.levels().iter().enumerate() {
            let level_extent = level.extent();
            quantized.clear();
            quantized.resize(level.data().voxel_count(), 0u16);
            let mut cursor = 0;
            level.data().for_each_unit(volume.intensity(), |unit| {
                quantized[cursor] = f16::from_f32(unit).to_bits();
                cursor += 1;
            });
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: index as u32,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                bytemuck::cast_slice(&quantized),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level_extent.x() * 2),
                    rows_per_image: Some(level_extent.y()),
                },
                Extent3d {
                    width: level_extent.x(),
                    height: level_extent.y(),
                    depth_or_array_layers: level_extent.z(),
                },
            );
        }

        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("grana.volume.view"),
            dimension: Some(TextureViewDimension::D3),
            ..Default::default()
        });

        Self { view, level_count }
    }

    pub fn view(&self) -> &TextureView {
        &self.view
    }

    pub fn level_count(&self) -> u32 {
        self.level_count
    }
}
