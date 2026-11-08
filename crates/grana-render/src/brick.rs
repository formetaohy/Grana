use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level};
use std::collections::HashSet;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages, Device,
    Queue, util::DeviceExt,
};

use crate::atlas::Atlas;
use crate::summary::Summary;
use grana_volume::CHANNEL_LIMIT;

const MAXIMA_PLANE: usize = CHANNEL_LIMIT / 4;

pub(crate) const HOLLOW_FLAG: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct BrickRecord {
    pub slot: u32,
    pub flags: u32,
    pub time: u32,
    pub reserved: u32,
    pub maxima: [[f32; 4]; MAXIMA_PLANE],
}

const _: () = {
    assert!(size_of::<BrickRecord>() == 80);
    assert!(std::mem::offset_of!(BrickRecord, maxima) == 16);
};

impl BrickRecord {
    pub const MISSING: Self = Self {
        slot: 0,
        flags: 0,
        time: u32::MAX,
        reserved: 0,
        maxima: [[0.0; 4]; MAXIMA_PLANE],
    };

    pub const fn hollow(time: u32) -> Self {
        Self {
            slot: 0,
            flags: HOLLOW_FLAG,
            time,
            reserved: 0,
            maxima: [[0.0; 4]; MAXIMA_PLANE],
        }
    }

    pub fn new(slot: u32, time: u32, maxima: [[f32; 4]; MAXIMA_PLANE]) -> Self {
        Self {
            slot,
            flags: 0,
            time,
            reserved: 0,
            maxima,
        }
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn is_live(&self) -> bool {
        self.time != u32::MAX
    }
}

pub(crate) struct BrickTable {
    pub grid: BrickGrid,
    pub origin: Vec3,
    pub world: Vec3,
    pub dims: Vec3,
    pub voxel: Vec3,
    pub atlas: [u32; 3],
    level: usize,
    time: Option<u32>,
    records: Vec<BrickRecord>,
    live: HashSet<u32>,
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
        summary: &Summary,
    ) -> Self {
        let grid = BrickGrid::new(source.extent());
        let records = vec![BrickRecord::MISSING; grid.count()];
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.brick.records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.level.group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: summary.buffer().as_entire_binding(),
                },
            ],
        });
        let world = Vec3::from(source.world_extent());
        let origin = Vec3::from(source.origin());
        let extent = source.extent();
        let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let voxel = world / dims;
        Self {
            grid,
            origin,
            world,
            dims,
            voxel,
            atlas,
            level,
            time: None,
            records,
            live: HashSet::new(),
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

    pub fn rebase(&mut self, queue: &Queue, time: u32, atlas: &Atlas) {
        let residents = atlas.residents(time, self.level);
        let mut settled = HashSet::with_capacity(residents.len());
        for resident in &residents {
            let record = match resident.slot {
                Some(slot) => BrickRecord::new(slot + 1, time, resident.maxima),
                None => BrickRecord::hollow(time),
            };
            if self.records[resident.index] != record {
                self.set_record(queue, resident.index, record);
            }
            settled.insert(resident.index as u32);
        }
        for index in std::mem::take(&mut self.live) {
            if settled.contains(&index) {
                continue;
            }
            if self.records[index as usize].is_live() {
                self.set_record(queue, index as usize, BrickRecord::MISSING);
            }
        }
        self.live = settled;
        self.time = Some(time);
    }

    pub fn set_record(&mut self, queue: &Queue, index: usize, record: BrickRecord) {
        self.records[index] = record;
        if record.is_live() {
            self.live.insert(index as u32);
        } else {
            self.live.remove(&(index as u32));
        }
        queue.write_buffer(&self.buffer, offset(index), bytemuck::bytes_of(&record));
    }

    pub fn drop_record(&mut self, queue: &Queue, index: usize, time: u32) {
        if self.records[index].time() == time {
            self.set_record(queue, index, BrickRecord::MISSING);
        }
    }

    pub fn time(&self) -> Option<u32> {
        self.time
    }
}

fn offset(index: usize) -> u64 {
    (index * size_of::<BrickRecord>()) as u64
}
