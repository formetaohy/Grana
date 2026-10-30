use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Buffer,
    BufferUsages, Device, Sampler, TextureView, util::DeviceExt,
};

use crate::atlas::Atlas;

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
    pub(crate) const EMPTY: Self = Self {
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

pub struct Resources<'a> {
    pub layout: &'a BindGroupLayout,
    pub uniforms: &'a Buffer,
    pub section_uniforms: &'a Buffer,
    pub lut: &'a TextureView,
    pub lut_sampler: &'a Sampler,
}

pub struct LevelCache {
    pub grid: BrickGrid,
    pub world: Vec3,
    pub voxel: Vec3,
    pub dims: Vec3,
    level: usize,
    time: Option<u32>,
    records: Vec<BrickRecord>,
    buffer: Buffer,
    group: BindGroup,
    section_group: BindGroup,
}

impl LevelCache {
    pub fn new(
        device: &Device,
        resources: &Resources<'_>,
        level_index: usize,
        level: Level,
    ) -> Self {
        let grid = BrickGrid::new(level.extent());
        let records = vec![BrickRecord::EMPTY; grid.count()];
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.brick.records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        let group = bind_group(
            device,
            resources.layout,
            resources.uniforms,
            &buffer,
            resources.lut,
            resources.lut_sampler,
        );
        let section_group = bind_group(
            device,
            resources.layout,
            resources.section_uniforms,
            &buffer,
            resources.lut,
            resources.lut_sampler,
        );
        let world = Vec3::from(level.world_extent());
        let extent = level.extent();
        let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let voxel = world / dims;
        Self {
            grid,
            world,
            voxel,
            dims,
            level: level_index,
            time: None,
            records,
            buffer,
            group,
            section_group,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn section_group(&self) -> &BindGroup {
        &self.section_group
    }

    pub fn sync(&mut self, queue: &wgpu::Queue, time: u32, atlas: &Atlas) {
        if self.time == Some(time) {
            return;
        }
        self.records.fill(BrickRecord::EMPTY);
        for (slot, index, maxima) in atlas.bricks(time, self.level) {
            self.records[index] = BrickRecord::new(slot + 1, maxima);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.records));
        self.time = Some(time);
    }

    pub fn set_record(
        &mut self,
        queue: &wgpu::Queue,
        time: u32,
        index: usize,
        record: BrickRecord,
    ) {
        if self.time != Some(time) {
            return;
        }
        self.records[index] = record;
        queue.write_buffer(&self.buffer, offset(index), bytemuck::bytes_of(&record));
    }

    pub fn clear_record(&mut self, queue: &wgpu::Queue, time: u32, index: usize) {
        self.set_record(queue, time, index, BrickRecord::EMPTY);
    }
}

fn offset(index: usize) -> u64 {
    (index * size_of::<BrickRecord>()) as u64
}

fn bind_group(
    device: &Device,
    layout: &BindGroupLayout,
    uniforms: &Buffer,
    records: &Buffer,
    lut: &TextureView,
    lut_sampler: &Sampler,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
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
                resource: records.as_entire_binding(),
            },
        ],
    })
}
