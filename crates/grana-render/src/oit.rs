use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device,
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::layout::Layouts;

const COUNT_BYTES: u64 = 4;
const KEY_BYTES: u64 = 4;
const COLOR_BYTES: u64 = 8;
const CLAIM_BYTES: u64 = 4;
const SLOT_BYTES: u64 = KEY_BYTES + COLOR_BYTES + CLAIM_BYTES;
const RESIDUAL_BYTES: u64 = 12;
const MINIMUM_LAYERS: u64 = 2;
const MAXIMUM_LAYERS: u64 = 8;
const PIXEL_STEP: u64 = 1 << 17;

pub(crate) const RESIDUAL_COLOR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const RESIDUAL_MOMENT_FORMAT: TextureFormat = TextureFormat::Rg16Float;

const RESIDUAL_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

pub(crate) fn residual_targets() -> [Option<ColorTargetState>; 2] {
    [
        Some(ColorTargetState {
            format: RESIDUAL_COLOR_FORMAT,
            blend: Some(RESIDUAL_BLEND),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState {
            format: RESIDUAL_MOMENT_FORMAT,
            blend: Some(RESIDUAL_BLEND),
            write_mask: ColorWrites::ALL,
        }),
    ]
}

pub(crate) struct Oit {
    keys: Buffer,
    colors: Buffer,
    counts: Buffer,
    claims: Buffer,
    residual_color: TextureView,
    residual_moment: TextureView,
    write_group: BindGroup,
    read_group: BindGroup,
    layers: u32,
    size: [u32; 2],
    capacity: u64,
}

impl Oit {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        size: [u32; 2],
        pixels: u64,
        budget: u64,
    ) -> Self {
        assert!(
            pixels > 0 && size[0] > 0 && size[1] > 0,
            "a fragment store covers a non empty view of a non empty surface"
        );
        let capacity = pixels
            .div_ceil(PIXEL_STEP)
            .checked_mul(PIXEL_STEP)
            .expect("a fragment store holds a finite view");
        let residual_bytes = u64::from(size[0]) * u64::from(size[1]) * RESIDUAL_BYTES;
        let layers = allocation(capacity, residual_bytes, budget);
        let slots = capacity * layers;
        let keys = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.keys"),
            size: (slots * KEY_BYTES).max(4),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let colors = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.colors"),
            size: (slots * COLOR_BYTES).max(8),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let claims = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.claims"),
            size: (slots * CLAIM_BYTES).max(4),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.counts"),
            size: (capacity * COUNT_BYTES).max(4),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let residual_color = residual(
            device,
            "grana.oit.residual.color",
            RESIDUAL_COLOR_FORMAT,
            size,
        );
        let residual_moment = residual(
            device,
            "grana.oit.residual.moment",
            RESIDUAL_MOMENT_FORMAT,
            size,
        );
        let write_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.oit.write.group"),
            layout: &layouts.oit_write,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: keys.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: colors.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: counts.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: claims.as_entire_binding(),
                },
            ],
        });
        let read_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.oit.read.group"),
            layout: &layouts.oit_read,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: keys.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: colors.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: counts.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&residual_color),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&residual_moment),
                },
            ],
        });
        Self {
            keys,
            colors,
            counts,
            claims,
            residual_color,
            residual_moment,
            write_group,
            read_group,
            layers: layers as u32,
            size,
            capacity,
        }
    }

    pub fn fits(&self, size: [u32; 2], pixels: u64) -> bool {
        self.size == size && pixels <= self.capacity
    }

    pub fn layers(&self) -> u32 {
        self.layers
    }

    pub fn write_group(&self) -> &BindGroup {
        &self.write_group
    }

    pub fn read_group(&self) -> &BindGroup {
        &self.read_group
    }

    pub fn residual_color(&self) -> &TextureView {
        &self.residual_color
    }

    pub fn residual_moment(&self) -> &TextureView {
        &self.residual_moment
    }

    pub fn clear(&self, encoder: &mut CommandEncoder) {
        encoder.clear_buffer(&self.keys, 0, None);
        encoder.clear_buffer(&self.colors, 0, None);
        encoder.clear_buffer(&self.counts, 0, None);
        encoder.clear_buffer(&self.claims, 0, None);
    }
}

fn allocation(capacity: u64, residual: u64, budget: u64) -> u64 {
    let fixed = capacity * COUNT_BYTES + residual;
    let slot = capacity * SLOT_BYTES;
    let floor = fixed + MINIMUM_LAYERS * slot;
    let budget = budget.max(floor);
    ((budget - fixed) / slot).clamp(MINIMUM_LAYERS, MAXIMUM_LAYERS)
}

fn residual(
    device: &Device,
    label: &'static str,
    format: TextureFormat,
    size: [u32; 2],
) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
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
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default())
}
