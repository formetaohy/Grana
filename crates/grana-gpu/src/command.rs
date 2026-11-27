use bytemuck::bytes_of;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

const REGION_LIMIT: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct IndexedArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

pub struct MultiDraw {
    args: Buffer,
    counts: Buffer,
    stride: u64,
    regions: usize,
    capacity: u32,
}

impl MultiDraw {
    pub fn new(device: &Device, label: &'static str, regions: usize, capacity: u32) -> Self {
        assert!(
            (1..=REGION_LIMIT).contains(&regions),
            "a multi draw holds one to {REGION_LIMIT} regions"
        );
        assert!(capacity > 0, "a multi draw holds one row per object");
        let stride = size_of::<IndexedArgs>() as u64;
        let args = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: stride * capacity as u64 * regions as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let counts = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: (regions * size_of::<u32>()) as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            args,
            counts,
            stride,
            regions,
            capacity,
        }
    }

    pub fn args(&self) -> &Buffer {
        &self.args
    }

    pub fn counts(&self) -> &Buffer {
        &self.counts
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn region(&self, region: usize) -> u64 {
        assert!(
            region < self.regions,
            "a draw region lies inside the multi draw"
        );
        self.stride * self.capacity as u64 * region as u64
    }

    pub fn count(&self, region: usize) -> u64 {
        assert!(
            region < self.regions,
            "a draw region lies inside the multi draw"
        );
        size_of::<u32>() as u64 * region as u64
    }

    pub fn reset(&self, queue: &Queue) {
        const ZERO: [u8; REGION_LIMIT * size_of::<u32>()] = [0; REGION_LIMIT * size_of::<u32>()];
        queue.write_buffer(&self.counts, 0, &ZERO[..self.regions * size_of::<u32>()]);
    }

    pub fn bytes(&self) -> u64 {
        self.args.size() + self.counts.size()
    }
}

pub struct InstanceCommands {
    buffer: Buffer,
}

impl InstanceCommands {
    pub fn new(device: &Device, label: &'static str) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: size_of::<[u32; 4]>() as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer }
    }

    pub fn reset(&self, queue: &Queue, vertex_count: u32) {
        queue.write_buffer(&self.buffer, 0, bytes_of(&[vertex_count, 0, 0, 0]));
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }
}
