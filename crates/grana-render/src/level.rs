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
    records: Vec<BrickRecord>,
    buffer: Buffer,
    group: BindGroup,
    section_group: BindGroup,
}

impl LevelCache {
    pub fn new(device: &Device, resources: &Resources<'_>, level: Level) -> Self {
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
