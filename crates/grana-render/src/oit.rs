use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferUsages, CommandEncoder, Device,
};

use crate::layout::Layouts;

const FRAGMENTS_PER_PIXEL: u64 = 8;
const NODE_BYTES: u64 = 16;
const NODE_LIMIT: u64 = 64 << 20;

pub(crate) struct Oit {
    head: Buffer,
    counter: Buffer,
    write_group: BindGroup,
    read_group: BindGroup,
    pixels: u32,
}

impl Oit {
    pub fn new(device: &Device, layouts: &Layouts, region: [u32; 4]) -> Self {
        assert!(
            region[2] > 0 && region[3] > 0,
            "a fragment store covers a non empty pane"
        );
        let pixels = region[2] * region[3];
        let capacity = (u64::from(pixels) * FRAGMENTS_PER_PIXEL).clamp(1, NODE_LIMIT / NODE_BYTES);
        let head = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.head"),
            size: u64::from(pixels) * size_of::<u32>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let nodes = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.nodes"),
            size: capacity * NODE_BYTES,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let counter = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.oit.counter"),
            size: size_of::<u32>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let write_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.oit.write.group"),
            layout: &layouts.oit_write,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: nodes.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: head.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: counter.as_entire_binding(),
                },
            ],
        });
        let read_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.oit.read.group"),
            layout: &layouts.oit_read,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: head.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: nodes.as_entire_binding(),
                },
            ],
        });
        Self {
            head,
            counter,
            write_group,
            read_group,
            pixels,
        }
    }

    pub fn pixels(&self) -> u32 {
        self.pixels
    }

    pub fn write_group(&self) -> &BindGroup {
        &self.write_group
    }

    pub fn read_group(&self) -> &BindGroup {
        &self.read_group
    }

    pub fn clear(&self, encoder: &mut CommandEncoder) {
        encoder.clear_buffer(&self.head, 0, None);
        encoder.clear_buffer(&self.counter, 0, None);
    }
}
