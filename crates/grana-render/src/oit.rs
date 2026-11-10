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
const TAIL_BYTES: u64 = 20;
const DROPPED_BYTES: u64 = 4;
const BYTES_PER_PIXEL: u64 = 76;
const MINIMUM_LAYERS: u64 = 2;
const MAXIMUM_LAYERS: u64 = 8;
const PIXEL_STEP: u64 = 1 << 17;
const TAIL_PIXELS: u64 = 1 << 20;
const TAIL_PIXELS_COARSE: u64 = 1 << 22;

pub(crate) const ACCUMULATE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const REVEAL_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const TAIL_DEPTH_FORMAT: TextureFormat = TextureFormat::Rg16Float;
pub(crate) const COVERAGE_FORMAT: TextureFormat = TextureFormat::R8Uint;

const ACCUMULATE_BLEND: BlendState = BlendState {
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

const REVEAL_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

const NEAREST_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Min,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Min,
    },
};

pub(crate) fn tail_targets() -> [Option<ColorTargetState>; 3] {
    [
        Some(ColorTargetState {
            format: ACCUMULATE_FORMAT,
            blend: Some(ACCUMULATE_BLEND),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState {
            format: REVEAL_FORMAT,
            blend: Some(REVEAL_BLEND),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState {
            format: TAIL_DEPTH_FORMAT,
            blend: Some(NEAREST_BLEND),
            write_mask: ColorWrites::ALL,
        }),
    ]
}

pub(crate) struct Oit {
    keys: Buffer,
    colors: Buffer,
    counts: Buffer,
    dropped: Buffer,
    accumulate: TextureView,
    reveal: TextureView,
    tail_depth: TextureView,
    coverage: TextureView,
    write_group: BindGroup,
    coverage_group: BindGroup,
    read_group: BindGroup,
    layers: u32,
    size: [u32; 2],
    capacity: u64,
    tail_scale: u32,
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
        let surface = u64::from(size[0]) * u64::from(size[1]);
        let tail_scale = tail_scale(surface);
        let tail_size = [size[0].div_ceil(tail_scale), size[1].div_ceil(tail_scale)];
        let tail_pixels = u64::from(tail_size[0]) * u64::from(tail_size[1]);
        let (layers, _) = allocation(tail_pixels, capacity, budget);
        let layers = layers as u32;
        let slots = capacity * u64::from(layers);
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
        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.counts"),
            size: (capacity * COUNT_BYTES).max(4),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dropped = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.dropped"),
            size: DROPPED_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let accumulate = tail(device, "grana.oit.accumulate", ACCUMULATE_FORMAT, tail_size);
        let reveal = tail(device, "grana.oit.reveal", REVEAL_FORMAT, tail_size);
        let tail_depth = tail(device, "grana.oit.tail.depth", TAIL_DEPTH_FORMAT, tail_size);
        let coverage = coverage(device, size);
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
                    resource: dropped.as_entire_binding(),
                },
            ],
        });
        let coverage_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.oit.coverage.group"),
            layout: &layouts.oit_coverage,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&coverage),
            }],
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
                    resource: wgpu::BindingResource::TextureView(&accumulate),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&reveal),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&tail_depth),
                },
            ],
        });
        Self {
            keys,
            colors,
            counts,
            dropped,
            accumulate,
            reveal,
            tail_depth,
            coverage,
            write_group,
            coverage_group,
            read_group,
            layers,
            size,
            capacity,
            tail_scale,
        }
    }

    pub fn fits(&self, size: [u32; 2], pixels: u64) -> bool {
        self.size == size && pixels <= self.capacity
    }

    pub fn tail_size(&self, region: [u32; 4]) -> [u32; 2] {
        [
            region[2].div_ceil(self.tail_scale),
            region[3].div_ceil(self.tail_scale),
        ]
    }

    pub fn layers(&self) -> u32 {
        self.layers
    }

    pub fn tail_scale(&self) -> u32 {
        self.tail_scale
    }

    pub fn dropped(&self) -> &Buffer {
        &self.dropped
    }

    pub fn write_group(&self) -> &BindGroup {
        &self.write_group
    }

    pub fn coverage_group(&self) -> &BindGroup {
        &self.coverage_group
    }

    pub fn read_group(&self) -> &BindGroup {
        &self.read_group
    }

    pub fn accumulate(&self) -> &TextureView {
        &self.accumulate
    }

    pub fn reveal(&self) -> &TextureView {
        &self.reveal
    }

    pub fn tail_depth(&self) -> &TextureView {
        &self.tail_depth
    }

    pub fn coverage(&self) -> &TextureView {
        &self.coverage
    }

    pub fn clear(&self, encoder: &mut CommandEncoder) {
        encoder.clear_buffer(&self.keys, 0, None);
        encoder.clear_buffer(&self.colors, 0, None);
        encoder.clear_buffer(&self.counts, 0, None);
        encoder.clear_buffer(&self.dropped, 0, None);
    }
}

fn allocation(tail_pixels: u64, capacity: u64, budget: u64) -> (u64, u64) {
    let tail_bytes = tail_pixels * TAIL_BYTES;
    let fixed = tail_bytes + capacity * COUNT_BYTES;
    let layer_bytes = capacity * (KEY_BYTES + COLOR_BYTES);
    let floor = fixed + MINIMUM_LAYERS * layer_bytes;
    let budget = budget.max(capacity * BYTES_PER_PIXEL).max(floor);
    let layers = ((budget - fixed) / layer_bytes).clamp(MINIMUM_LAYERS, MAXIMUM_LAYERS);
    (layers, fixed + layers * layer_bytes)
}

fn tail_scale(pixels: u64) -> u32 {
    if pixels <= TAIL_PIXELS {
        1
    } else if pixels <= TAIL_PIXELS_COARSE {
        2
    } else {
        4
    }
}

fn tail(
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

fn coverage(device: &Device, size: [u32; 2]) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some("grana.oit.coverage"),
            size: Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: COVERAGE_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default())
}
