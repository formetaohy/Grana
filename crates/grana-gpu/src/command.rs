use bytemuck::bytes_of;
use wgpu::{Buffer, BufferUsages, Device, Queue};

#[repr(C)]
#[derive(Clone, Copy)]
struct IndexedArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

pub struct IndexedCommands {
    buffer: Buffer,
    count: u32,
    stride: u64,
}

impl IndexedCommands {
    pub fn new(device: &Device, label: &'static str, count: usize, regions: usize) -> Self {
        assert!(
            count > 0,
            "an indirect draw table holds at least one command"
        );
        assert!(regions > 0, "an indirect draw table holds a region");
        let stride = (count * size_of::<IndexedArgs>()) as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: stride * regions as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            count: count as u32,
            stride,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn offset(&self, region: usize) -> u64 {
        self.stride * region as u64
    }
}

pub struct InstanceCommands {
    buffer: Buffer,
}

impl InstanceCommands {
    pub fn new(device: &Device, label: &'static str) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
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
