use bytemuck::cast_slice;
use grana_gpu::Table;
use grana_volume::{CHANNEL_LIMIT, Intensity, PACK, PACK_LIMIT, Pyramid};
use half::f16;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferUsages, Device,
    Queue, Sampler, SamplerDescriptor, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor, util::DeviceExt,
};

use crate::frame::FrameData;
use crate::layout::Layouts;
use crate::schema::ChannelRow;
use crate::upload::Upload;
use grana_display::{TransferFunction, VolumeMode};

const LUT_SIZE: usize = 1024;

const TRANSPARENT_ALPHA: f32 = 0.999;

const MINIMUM_ALPHA: f32 = 1.0e-4;

const NEUTRAL_WORDS: usize = 108;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ChannelData {
    opacity: f32,
    start: f32,
    inverse: f32,
    lut_row: f32,
    threshold: f32,
    mode: f32,
    nearest: f32,
    material: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct NeutralData {
    words: [u32; NEUTRAL_WORDS],
}

const _: () = {
    assert!(size_of::<ChannelData>() == 32);
    assert!(size_of::<NeutralData>() == 432);
};

pub(crate) fn bytes(channels: usize) -> u64 {
    let frames = 2 * size_of::<FrameData>() as u64;
    let neutral = size_of::<NeutralData>() as u64;
    let rows = 2 * (CHANNEL_LIMIT * size_of::<ChannelData>()) as u64;
    let lut = LUT_SIZE as u64 * channels as u64 * size_of::<[u16; 4]>() as u64;
    frames + neutral + rows + lut
}

pub(crate) struct SceneTable {
    frame: Buffer,
    channels: Table<ChannelData>,
    shadow_frame: Buffer,
    shadow_channels: Table<ChannelData>,
    shadow_group: BindGroup,
    lut: Texture,
    transfers: Vec<Option<TransferFunction>>,
    thresholds: Vec<Option<f32>>,
    floor: f32,
    packs: [u32; 4],
    quality: f32,
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
        let frame = uniform_buffer(device, "grana.frame.uniforms", size_of::<FrameData>());
        let neutral = NeutralData {
            words: [0; NEUTRAL_WORDS],
        };
        let neutral = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.neutral.uniforms"),
            contents: bytemuck::bytes_of(&neutral),
            usage: BufferUsages::UNIFORM,
        });
        let table = Table::with_capacity(device, "grana.channels", CHANNEL_LIMIT);
        let lut = transfer_texture(device, "grana.transfer.lut", channels);
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.lut.view"),
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
        let binding = SceneBinding {
            frame: &frame,
            neutral: &neutral,
            channels: table.buffer(),
            sampler: &lut_sampler,
            extinction: &lut_view,
        };
        let group = scene_group(device, layouts, &binding);
        let shadow_frame = uniform_buffer(
            device,
            "grana.shadow.frame.uniforms",
            size_of::<FrameData>(),
        );
        let shadow_channels = Table::with_capacity(device, "grana.shadow.channels", CHANNEL_LIMIT);
        let shadow_binding = SceneBinding {
            frame: &shadow_frame,
            neutral: &neutral,
            channels: shadow_channels.buffer(),
            sampler: &lut_sampler,
            extinction: &lut_view,
        };
        let shadow_group = scene_group(device, layouts, &shadow_binding);
        Self {
            frame,
            channels: table,
            shadow_frame,
            shadow_channels,
            shadow_group,
            lut,
            transfers: vec![None; channels],
            thresholds: vec![None; channels],
            floor: 0.0,
            packs: [0; 4],
            quality: 1.0,
            group,
            channels_count: channels,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn bytes(&self) -> u64 {
        bytes(self.channels_count)
    }

    pub fn shadow_group(&self) -> &BindGroup {
        &self.shadow_group
    }

    pub fn publish(
        &mut self,
        device: &Device,
        queue: &Queue,
        upload: &mut Upload,
        pyramid: &Pyramid,
        rows: &[ChannelRow],
    ) {
        assert_eq!(
            rows.len(),
            self.channels_count,
            "the scene carries one row per channel"
        );
        for (index, row) in rows.iter().enumerate() {
            if self.transfers[index].as_ref() != Some(&row.transfer) {
                self.write_transfer(upload, index, &row.transfer);
                self.transfers[index] = Some(row.transfer.clone());
            }
        }
        let data = channel_data(rows, &self.thresholds, pyramid);
        let _ = self.shadow_channels.write(device, queue, &data.rows);
        self.channels.write(device, queue, &data.rows);
        self.floor = data.floor;
        self.packs = data.packs;
        self.quality = data.quality;
    }

    pub fn threshold_floor(&self) -> f32 {
        self.floor
    }

    pub fn samples_per_voxel(&self) -> f32 {
        self.quality
    }

    pub fn packs(&self) -> [u32; 4] {
        self.packs
    }

    pub fn write_shadow_frame(&self, queue: &Queue, frame: FrameData) {
        queue.write_buffer(&self.shadow_frame, 0, bytemuck::bytes_of(&frame));
    }

    pub fn write_frame(&self, queue: &Queue, frame: FrameData) {
        queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
    }

    fn write_transfer(&mut self, upload: &mut Upload, row: usize, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE);
        let mut extinction = Vec::with_capacity(lut.len() * 4);
        for [red, green, blue, alpha] in lut.iter() {
            let opacity = alpha.clamp(0.0, TRANSPARENT_ALPHA);
            for channel in [*red, *green, *blue, -(1.0 - opacity).ln()] {
                extinction.push(f16::from_f32(channel).to_bits());
            }
        }
        upload.texture(
            &self.lut,
            [0, row as u32, 0],
            [LUT_SIZE as u32, 1, 1],
            (LUT_SIZE as u32) * 8,
            1,
            cast_slice(&extinction),
        );
        self.thresholds[row] = lut
            .iter()
            .position(|stop| stop[3] > MINIMUM_ALPHA)
            .map(|index| index as f32 / LUT_SIZE as f32);
    }
}

struct SceneBinding<'a> {
    frame: &'a Buffer,
    neutral: &'a Buffer,
    channels: &'a Buffer,
    sampler: &'a Sampler,
    extinction: &'a TextureView,
}

fn scene_group(device: &Device, layouts: &Layouts, binding: &SceneBinding<'_>) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.scene.group"),
        layout: &layouts.scene,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: binding.frame.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: binding.neutral.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: binding.channels.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::Sampler(binding.sampler),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(binding.extinction),
            },
        ],
    })
}

fn uniform_buffer(device: &Device, label: &'static str, size: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

struct ChannelTable {
    rows: Vec<ChannelData>,
    floor: f32,
    packs: [u32; 4],
    quality: f32,
}

fn channel_data(
    rows: &[ChannelRow],
    thresholds: &[Option<f32>],
    pyramid: &Pyramid,
) -> ChannelTable {
    let mut floor = f32::INFINITY;
    let mut channels = 0u32;
    let mut peaks = 0u32;
    let rows = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            assert!(
                (0.0..=1.0).contains(&row.opacity),
                "a channel opacity lies in the unit interval"
            );
            let info = &pyramid.channels()[index];
            let categorical = crate::material::categorical(info);
            let range = info.intensity();
            let (start, inverse) = window(range, row.intensity);
            let opaque = thresholds[index];
            let mode = row.volume.mode();
            let visible =
                opaque.map_or(f32::INFINITY, |position| unit_at(start, inverse, position));
            let threshold = match mode {
                VolumeMode::Isosurface if categorical => visible,
                VolumeMode::Isosurface => unit_at(start, inverse, row.volume.isovalue()),
                VolumeMode::Blend | VolumeMode::Maximum => match opaque {
                    Some(position) if position <= 0.0 => f32::NEG_INFINITY,
                    Some(_) => visible,
                    None => f32::INFINITY,
                },
            };
            let opacity = if row.visible { row.opacity } else { 0.0 };
            if opacity > 0.0 {
                channels |= 1 << index;
                let reach = match mode {
                    VolumeMode::Maximum => {
                        peaks |= 1 << index;
                        start
                    }
                    VolumeMode::Blend | VolumeMode::Isosurface => threshold,
                };
                floor = floor.min(reach);
            }
            ChannelData {
                opacity,
                start,
                inverse,
                lut_row: (index as f32 + 0.5) / rows.len() as f32,
                threshold,
                mode: mode.index() as f32,
                nearest: f32::from(u8::from(categorical)),
                material: f32::from(u8::from(categorical)),
            }
        })
        .collect();
    let packs = (0..PACK_LIMIT).fold(0u32, |mask, pack| {
        let members = (0..PACK).fold(0u32, |bits, component| {
            bits | ((channels >> (pack * PACK + component)) & 1)
        });
        mask | (u32::from(members != 0) << pack)
    });
    ChannelTable {
        rows,
        floor,
        packs: [channels, packs, peaks, 0],
        quality: crate::material::samples_per_voxel(pyramid),
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

fn unit_at(start: f32, inverse: f32, position: f32) -> f32 {
    if inverse > 0.0 {
        start + position / inverse
    } else {
        f32::INFINITY
    }
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
