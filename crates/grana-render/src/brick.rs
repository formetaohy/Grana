use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages, Device,
    Queue, util::DeviceExt,
};

use crate::CHANNEL_LIMIT;
use crate::atlas::Atlas;

const MAXIMA_PLANE: usize = CHANNEL_LIMIT / 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct BrickRecord {
    pub slot: u32,
    pub flags: u32,
    pub reserved: [u32; 2],
    pub maxima: [[f32; 4]; MAXIMA_PLANE],
}

const _: () = {
    assert!(size_of::<BrickRecord>() == 80);
    assert!(std::mem::offset_of!(BrickRecord, maxima) == 16);
};

impl BrickRecord {
    pub const EMPTY: Self = Self {
        slot: 0,
        flags: 0,
        reserved: [0; 2],
        maxima: [[0.0; 4]; MAXIMA_PLANE],
    };

    pub fn new(slot: u32, maxima: [[f32; 4]; MAXIMA_PLANE]) -> Self {
        Self {
            slot,
            flags: 0,
            reserved: [0; 2],
            maxima,
        }
    }
}

pub(crate) struct BrickTable {
    pub grid: BrickGrid,
    pub world: Vec3,
    pub dims: Vec3,
    pub voxel: Vec3,
    pub atlas: [u32; 3],
    level: usize,
    time: Option<u32>,
    records: Vec<BrickRecord>,
    buffer: Buffer,
    group: BindGroup,
}

impl BrickTable {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        level: usize,
        source: Level,
        atlas: [u32; 3],
    ) -> Self {
        let grid = BrickGrid::new(source.extent());
        let records = vec![BrickRecord::EMPTY; grid.count()];
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.brick.records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.level.group"),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let world = Vec3::from(source.world_extent());
        let extent = source.extent();
        let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let voxel = world / dims;
        Self {
            grid,
            world,
            dims,
            voxel,
            atlas,
            level,
            time: None,
            records,
            buffer,
            group,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn sync(&mut self, queue: &Queue, time: u32, atlas: &Atlas) {
        self.records.fill(BrickRecord::EMPTY);
        for (slot, index, maxima) in atlas.bricks(time, self.level) {
            self.records[index] = BrickRecord::new(slot + 1, maxima);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.records));
        self.time = Some(time);
    }

    pub fn set_record(&mut self, queue: &Queue, index: usize, record: BrickRecord) {
        self.records[index] = record;
        queue.write_buffer(&self.buffer, offset(index), bytemuck::bytes_of(&record));
    }

    pub fn time(&self) -> Option<u32> {
        self.time
    }
}

fn offset(index: usize) -> u64 {
    (index * size_of::<BrickRecord>()) as u64
}
