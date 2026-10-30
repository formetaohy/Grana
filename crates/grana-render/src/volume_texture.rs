use grana_volume::Channel;
use half::f16;
use wgpu::{
    Device, Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension,
};

const UPLOAD_SLAB_BYTES: usize = 16 << 20;

pub struct ChannelTexture {
    view: TextureView,
    level_count: u32,
}

impl ChannelTexture {
    pub fn new(device: &Device, queue: &Queue, channel: &Channel) -> Self {
        let levels = channel.levels();
        let extent = levels[0].extent();
        let level_count = channel.level_count();
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.channel"),
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

        for (index, level) in levels.iter().enumerate() {
            let width = level.extent().x() as usize;
            let height = level.extent().y() as usize;
            let depth = level.extent().z() as usize;
            let mut quantized = Vec::with_capacity(width * height * depth);
            level.data().for_each_unit(channel.window(), |unit| {
                quantized.push(f16::from_f32(unit).to_bits());
            });
            let slab_depth =
                (UPLOAD_SLAB_BYTES / (width * height * size_of::<u16>())).clamp(1, depth);
            for z in (0..depth).step_by(slab_depth) {
                let rows = slab_depth.min(depth - z);
                let start = z * width * height;
                let end = (z + rows) * width * height;
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: index as u32,
                        origin: Origin3d {
                            x: 0,
                            y: 0,
                            z: z as u32,
                        },
                        aspect: TextureAspect::All,
                    },
                    bytemuck::cast_slice(&quantized[start..end]),
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some((width * size_of::<u16>()) as u32),
                        rows_per_image: Some(height as u32),
                    },
                    Extent3d {
                        width: width as u32,
                        height: height as u32,
                        depth_or_array_layers: rows as u32,
                    },
                );
            }
        }

        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("grana.channel.view"),
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
