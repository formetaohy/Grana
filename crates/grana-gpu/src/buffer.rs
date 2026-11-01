use bytemuck::Pod;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device};

pub fn storage<T: Pod>(device: &Device, label: &str, capacity: usize) -> Buffer {
    assert!(capacity > 0, "a storage buffer holds at least one row");
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size: (capacity * size_of::<T>()) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
