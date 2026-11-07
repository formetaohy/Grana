use bytemuck::cast_slice;
use grana_gpu::Table;
use grana_volume::{Intensity, Pyramid};
use half::f16;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferUsages, Device,
    Queue, SamplerDescriptor, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};

use crate::frame::{FrameData, SectionData};
use crate::layout::Layouts;
use crate::schema::ChannelRow;
use grana_display::TransferFunction;
use grana_volume::CHANNEL_LIMIT;

const LUT_SIZE: usize = 1024;

const TRANSPARENT_ALPHA: f32 = 0.999;

const MINIMUM_ALPHA: f32 = 1.0e-4;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ChannelData {
    opacity: f32,
    start: f32,
    inverse: f32,
    lut_row: f32,
    threshold: f32,
}

const _: () = assert!(size_of::<ChannelData>() == 20);

pub(crate) struct SceneTable {
    uniforms: Buffer,
    section_uniforms: Buffer,
    channels: Table<ChannelData>,
    lut: Texture,
    extinction: Texture,
    transfers: Vec<Option<TransferFunction>>,
    thresholds: Vec<Option<f32>>,
    group: BindGroup,
    channels_count: usize,
}

impl SceneTable {
    pub fn new(device: &Device, layouts: &Layouts, channels: usize) -> Self {
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a scene binds one to {} channels",
            CHANNEL_LIMIT
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.frame.uniforms"),
            size: size_of::<FrameData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let section_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.section.uniforms"),
            size: size_of::<SectionData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let table = Table::with_capacity(device, "grana.channels", CHANNEL_LIMIT);
        let lut = transfer_texture(device, "grana.transfer.lut", channels);
        let extinction = transfer_texture(device, "grana.transfer.extinction", channels);
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.lut.view"),
            ..Default::default()
        });
        let extinction_view = extinction.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.extinction.view"),
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.transfer.lut.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.scene.group"),
            layout: &layouts.scene,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: section_uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: table.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&lut_view),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::Sampler(&lut_sampler),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(&extinction_view),
                },
            ],
        });
        Self {
            uniforms,
            section_uniforms,
            channels: table,
            lut,
            extinction,
            transfers: vec![None; channels],
            thresholds: vec![None; channels],
            group,
            channels_count: channels,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn write(
        &mut self,
        device: &Device,
        queue: &Queue,
        pyramid: &Pyramid,
        rows: &[ChannelRow<'_>],
        frame: FrameData,
        section: SectionData,
    ) {
        assert_eq!(
            rows.len(),
            self.channels_count,
            "the scene carries one row per channel"
        );
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&frame));
        queue.write_buffer(&self.section_uniforms, 0, bytemuck::bytes_of(&section));
        let mut data = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(&row.opacity),
                "a channel opacity lies in the unit interval"
            );
            let range = pyramid.channels()[index].intensity();
            let (start, inverse) = window(range, row.window);
            if self.transfers[index].as_ref() != Some(row.transfer) {
                self.write_transfer(queue, index, row.transfer);
                self.transfers[index] = Some(row.transfer.clone());
            }
            let opaque = self.thresholds[index];
            let threshold = match opaque {
                Some(position) if inverse > 0.0 => start + position / inverse,
                Some(position) if position <= 0.0 => f32::NEG_INFINITY,
                _ => f32::INFINITY,
            };
            data.push(ChannelData {
                opacity: if row.visible { row.opacity } else { 0.0 },
                start,
                inverse,
                lut_row: (index as f32 + 0.5) / rows.len() as f32,
                threshold,
            });
        }
        let _ = self.channels.write(device, queue, &data);
    }

    fn write_transfer(&mut self, queue: &Queue, row: usize, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE);
        let mut straight = Vec::with_capacity(lut.len() * 4);
        let mut extinction = Vec::with_capacity(lut.len() * 4);
        for [red, green, blue, alpha] in &lut {
            for channel in [*red, *green, *blue, *alpha] {
                straight.push(f16::from_f32(channel).to_bits());
            }
            let opacity = alpha.clamp(0.0, TRANSPARENT_ALPHA);
            for channel in [*red, *green, *blue, -(1.0 - opacity).ln()] {
                extinction.push(f16::from_f32(channel).to_bits());
            }
        }
        write_row(queue, &self.lut, row, &straight);
        write_row(queue, &self.extinction, row, &extinction);
        self.thresholds[row] = lut
            .iter()
            .position(|stop| stop[3] > MINIMUM_ALPHA)
            .map(|index| index as f32 / LUT_SIZE as f32);
    }
}

fn transfer_texture(device: &Device, label: &str, channels: usize) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: LUT_SIZE as u32,
            height: channels as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba16Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_row(queue: &Queue, texture: &Texture, row: usize, quantized: &[u16]) {
    queue.write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: row as u32,
                z: 0,
            },
            aspect: TextureAspect::All,
        },
        cast_slice(quantized),
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LUT_SIZE as u32 * 8),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: LUT_SIZE as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

fn window(range: Intensity, window: Intensity) -> (f32, f32) {
    let span = range.span();
    if span <= 0.0 {
        return (1.0, 0.0);
    }
    let start = (window.min() - range.min()) / span;
    let window_span = (window.max() - window.min()) / span;
    let inverse = if window_span > 0.0 {
        1.0 / window_span
    } else {
        0.0
    };
    (start, inverse)
}
