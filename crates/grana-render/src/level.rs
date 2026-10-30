use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Buffer,
    BufferUsages, Device, Sampler, TextureView, util::DeviceExt,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct BrickRecord {
    pub slot: u32,
    pub reserved: [u32; 3],
    pub maxima: [[f32; 4]; 2],
}

const _: () = {
    assert!(size_of::<BrickRecord>() == 48);
    assert!(std::mem::offset_of!(BrickRecord, maxima) == 16);
};

impl BrickRecord {
    const EMPTY: Self = Self {
        slot: 0,
        reserved: [0; 3],
        maxima: [[0.0; 4]; 2],
    };

    pub fn new(slot: u32, maxima: [[f32; 4]; 2]) -> Self {
        Self {
            slot,
            reserved: [0; 3],
            maxima,
        }
    }
}

pub struct LevelCache {
    pub grid: BrickGrid,
    pub world: Vec3,
    pub voxel: Vec3,
    pub dims: Vec3,
    records: Vec<BrickRecord>,
    buffer: Buffer,
    group: BindGroup,
}

impl LevelCache {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        uniforms: &Buffer,
        lut: &TextureView,
        lut_sampler: &Sampler,
        level: Level,
    ) -> Self {
        let grid = BrickGrid::new(level.extent());
        let records = vec![BrickRecord::EMPTY; grid.count()];
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.brick.records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.frame.group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(lut),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(lut_sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let world = Vec3::from(level.world_extent());
        let extent = level.extent();
        let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let voxel = world / dims;
        Self {
            grid,
            world,
            voxel,
            dims,
            records,
            buffer,
            group,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn slot(&self, index: usize) -> u32 {
        self.records[index].slot
    }

    pub fn set_record(&mut self, queue: &wgpu::Queue, index: usize, record: BrickRecord) {
        self.records[index] = record;
        queue.write_buffer(
            &self.buffer,
            (index * size_of::<BrickRecord>()) as u64,
            bytemuck::bytes_of(&record),
        );
    }
}
