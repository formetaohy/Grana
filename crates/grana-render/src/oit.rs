use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device,
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::fragment::Fragment;
use crate::layout::Layouts;

const EPOCH_LIMIT: u32 = 255;

pub(crate) struct Key {
    slot: u32,
    depth: u32,
}

impl Key {
    pub(crate) fn of(layers: u64) -> Self {
        let slot = u32::BITS
            - u32::try_from(layers)
                .expect("a fragment store names its layers in the key")
                .leading_zeros();
        let epoch = u32::BITS - EPOCH_LIMIT.leading_zeros();
        assert!(
            slot + epoch < u32::BITS,
            "a fragment key holds an epoch, a depth and a layer slot"
        );
        Self {
            slot,
            depth: u32::BITS - epoch - slot,
        }
    }
}

pub(crate) fn prelude() -> String {
    let key = Key::of(crate::fragment::LAYER_LIMIT);
    format!(
        "const MAXIMUM_ENTRIES: u32 = {}u;\nconst OIT_SLOT_BITS: u32 = {}u;\nconst OIT_DEPTH_BITS: u32 = {}u;\n",
        crate::fragment::LAYERS + 1,
        key.slot,
        key.depth,
    )
}

pub(crate) const RESIDUAL_COLOR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const RESIDUAL_MOMENT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

const RESIDUAL_ADD: BlendState = BlendState {
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

const RESIDUAL_MARK: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Max,
    },
};

pub(crate) fn residual_targets() -> [Option<ColorTargetState>; 2] {
    [
        Some(ColorTargetState {
            format: RESIDUAL_COLOR_FORMAT,
            blend: Some(RESIDUAL_ADD),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState {
            format: RESIDUAL_MOMENT_FORMAT,
            blend: Some(RESIDUAL_MARK),
            write_mask: ColorWrites::ALL,
        }),
    ]
}

pub(crate) struct Oit {
    keys: Buffer,
    _colors: Buffer,
    counts: Buffer,
    residual_color: TextureView,
    residual_moment: TextureView,
    write_group: BindGroup,
    read_group: BindGroup,
    fragment: Fragment,
    epoch: u32,
}

impl Oit {
    pub fn new(device: &Device, layouts: &Layouts, fragment: Fragment) -> Self {
        let size = fragment.size();
        let keys = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.keys"),
            size: fragment.keys().max(4),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let colors = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.colors"),
            size: fragment.colors().max(8),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.counts"),
            size: fragment.counts().max(4),
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
            _colors: colors,
            counts,
            residual_color,
            residual_moment,
            write_group,
            read_group,
            fragment,
            epoch: 0,
        }
    }

    pub fn fits(&self, fragment: Fragment) -> bool {
        self.fragment == fragment
    }

    pub fn layers(&self) -> u32 {
        self.fragment.layers()
    }

    pub fn bytes(&self) -> u64 {
        self.fragment.bytes()
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

    pub fn next_epoch(&self) -> u32 {
        self.epoch % EPOCH_LIMIT + 1
    }

    pub fn begin(&mut self, encoder: &mut CommandEncoder) -> u32 {
        let epoch = self.next_epoch();
        if epoch == 1 {
            encoder.clear_buffer(&self.keys, 0, None);
            encoder.clear_buffer(&self.counts, 0, None);
        }
        self.epoch = epoch;
        epoch
    }
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
