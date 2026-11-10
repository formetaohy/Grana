use bytemuck::cast_slice;
use grana_gpu::Table;
use grana_volume::{Intensity, Pyramid};
use half::f16;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferUsages, Device,
    Queue, Sampler, SamplerDescriptor, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::PANE_COUNT;
use crate::frame::{FrameData, SectionData};
use crate::layout::Layouts;
use crate::schema::ChannelRow;
use grana_display::{TransferFunction, VolumeMode};
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
    mode: f32,
    pad: [f32; 2],
}

const _: () = assert!(size_of::<ChannelData>() == 32);

pub(crate) struct SceneTable {
    frames: [Buffer; PANE_COUNT],
    sections: [Buffer; PANE_COUNT],
    channels: [Table<ChannelData>; PANE_COUNT],
    shadow_frame: Buffer,
    shadow_channels: Table<ChannelData>,
    shadow_group: BindGroup,
    lut: Texture,
    extinction: Texture,
    section: Texture,
    transfers: Vec<Option<TransferFunction>>,
    thresholds: Vec<Option<f32>>,
    groups: [BindGroup; PANE_COUNT],
    channels_count: usize,
}

impl SceneTable {
    pub fn new(device: &Device, layouts: &Layouts, channels: usize) -> Self {
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a scene binds one to {} channels",
            CHANNEL_LIMIT
        );
        let frames = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grana.frame.uniforms"),
                size: size_of::<FrameData>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let sections = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grana.section.uniforms"),
                size: size_of::<SectionData>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let table =
            std::array::from_fn(|_| Table::with_capacity(device, "grana.channels", CHANNEL_LIMIT));
        let lut = transfer_texture(device, "grana.transfer.lut", channels);
        let extinction = transfer_texture(device, "grana.transfer.extinction", channels);
        let section = transfer_texture(device, "grana.transfer.section", channels);
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.lut.view"),
            ..Default::default()
        });
        let extinction_view = extinction.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.extinction.view"),
            ..Default::default()
        });
        let section_view = section.create_view(&TextureViewDescriptor {
            label: Some("grana.transfer.section.view"),
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
        let groups = std::array::from_fn(|index| {
            scene_group(
                device,
                layouts,
                SceneBinding {
                    frame: &frames[index],
                    section: &sections[index],
                    channels: table[index].buffer(),
                    lut: &lut_view,
                    sampler: &lut_sampler,
                    extinction: &extinction_view,
                    section_lut: &section_view,
                },
            )
        });
        let shadow_frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.shadow.frame.uniforms"),
            size: size_of::<FrameData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_channels = Table::with_capacity(device, "grana.shadow.channels", CHANNEL_LIMIT);
        let shadow_group = scene_group(
            device,
            layouts,
            SceneBinding {
                frame: &shadow_frame,
                section: &sections[0],
                channels: shadow_channels.buffer(),
                lut: &lut_view,
                sampler: &lut_sampler,
                extinction: &extinction_view,
                section_lut: &section_view,
            },
        );
        Self {
            frames,
            sections,
            channels: table,
            shadow_frame,
            shadow_channels,
            shadow_group,
            lut,
            extinction,
            section,
            transfers: vec![None; channels],
            thresholds: vec![None; channels],
            groups,
            channels_count: channels,
        }
    }

    pub fn group(&self, pane: usize) -> &BindGroup {
        assert!(pane < PANE_COUNT, "a scene table names one of the panes");
        &self.groups[pane]
    }

    pub fn shadow_group(&self) -> &BindGroup {
        &self.shadow_group
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
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
                self.write_transfer(queue, index, &row.transfer);
                self.transfers[index] = Some(row.transfer.clone());
            }
        }
        let data = channel_data(rows, &self.thresholds, pyramid);
        let _ = self.shadow_channels.write(device, queue, &data);
    }

    pub fn write_channels(
        &mut self,
        device: &Device,
        queue: &Queue,
        pyramid: &Pyramid,
        pane: usize,
        rows: &[ChannelRow],
    ) {
        assert!(pane < PANE_COUNT, "a scene table names one of the panes");
        assert_eq!(
            rows.len(),
            self.channels_count,
            "the scene carries one row per channel"
        );
        let data = channel_data(rows, &self.thresholds, pyramid);
        self.channels[pane].write(device, queue, &data);
    }

    pub fn write_shadow_frame(&mut self, queue: &Queue, frame: FrameData) {
        queue.write_buffer(&self.shadow_frame, 0, bytemuck::bytes_of(&frame));
    }

    pub fn write_frame(
        &mut self,
        pane: usize,
        queue: &Queue,
        frame: Option<FrameData>,
        section: Option<SectionData>,
    ) {
        assert!(pane < PANE_COUNT, "a scene table names one of the panes");
        if let Some(frame) = frame {
            queue.write_buffer(&self.frames[pane], 0, bytemuck::bytes_of(&frame));
        }
        if let Some(section) = section {
            queue.write_buffer(&self.sections[pane], 0, bytemuck::bytes_of(&section));
        }
    }

    fn write_transfer(&mut self, queue: &Queue, row: usize, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE);
        let section = transfer.section_lut(LUT_SIZE);
        let mut straight = Vec::with_capacity(lut.len() * 4);
        let mut extinction = Vec::with_capacity(lut.len() * 4);
        let mut coverage = Vec::with_capacity(lut.len() * 4);
        for (
            [red, green, blue, alpha],
            [section_red, section_green, section_blue, section_alpha],
        ) in lut.iter().zip(&section)
        {
            for channel in [*red, *green, *blue, *alpha] {
                straight.push(f16::from_f32(channel).to_bits());
            }
            let opacity = alpha.clamp(0.0, TRANSPARENT_ALPHA);
            for channel in [*red, *green, *blue, -(1.0 - opacity).ln()] {
                extinction.push(f16::from_f32(channel).to_bits());
            }
            for channel in [*section_red, *section_green, *section_blue, *section_alpha] {
                coverage.push(f16::from_f32(channel).to_bits());
            }
        }
        write_row(queue, &self.lut, row, &straight);
        write_row(queue, &self.extinction, row, &extinction);
        write_row(queue, &self.section, row, &coverage);
        self.thresholds[row] = lut
            .iter()
            .position(|stop| stop[3] > MINIMUM_ALPHA)
            .map(|index| index as f32 / LUT_SIZE as f32);
    }
}

struct SceneBinding<'a> {
    frame: &'a Buffer,
    section: &'a Buffer,
    channels: &'a Buffer,
    lut: &'a TextureView,
    sampler: &'a Sampler,
    extinction: &'a TextureView,
    section_lut: &'a TextureView,
}

fn scene_group(device: &Device, layouts: &Layouts, binding: SceneBinding<'_>) -> BindGroup {
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
                resource: binding.section.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: binding.channels.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::TextureView(binding.lut),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::Sampler(binding.sampler),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(binding.extinction),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::TextureView(binding.section_lut),
            },
        ],
    })
}

fn channel_data(
    rows: &[ChannelRow],
    thresholds: &[Option<f32>],
    pyramid: &Pyramid,
) -> Vec<ChannelData> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            assert!(
                (0.0..=1.0).contains(&row.opacity),
                "a channel opacity lies in the unit interval"
            );
            let range = pyramid.channels()[index].intensity();
            let (start, inverse) = window(range, row.window);
            let opaque = thresholds[index];
            let mode = row.volume.mode();
            let threshold = match mode {
                VolumeMode::Isosurface => unit_at(start, inverse, row.volume.isovalue()),
                VolumeMode::Blend | VolumeMode::Maximum => match opaque {
                    Some(position) if position <= 0.0 => f32::NEG_INFINITY,
                    Some(position) => unit_at(start, inverse, position),
                    None => f32::INFINITY,
                },
            };
            ChannelData {
                opacity: if row.visible { row.opacity } else { 0.0 },
                start,
                inverse,
                lut_row: (index as f32 + 0.5) / rows.len() as f32,
                threshold,
                mode: mode.index() as f32,
                pad: [0.0; 2],
            }
        })
        .collect()
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
