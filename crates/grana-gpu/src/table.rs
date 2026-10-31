use bytemuck::{Pod, cast_slice};
use wgpu::{Buffer, BufferUsages, Device, Queue};

pub struct Table<T: Pod> {
    label: &'static str,
    buffer: Buffer,
    capacity: usize,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Pod> Table<T> {
    pub fn new(device: &Device, label: &'static str) -> Self {
        Self::with_capacity(device, label, 1)
    }

    pub fn with_capacity(device: &Device, label: &'static str, capacity: usize) -> Self {
        assert!(capacity > 0, "a storage table holds at least one row");
        Self {
            label,
            buffer: storage_buffer::<T>(device, label, capacity),
            capacity,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn write(&mut self, device: &Device, queue: &Queue, rows: &[T]) -> bool {
        let grown = self.reserve(device, rows.len());
        if !rows.is_empty() {
            queue.write_buffer(&self.buffer, 0, cast_slice(rows));
        }
        grown
    }

    pub fn reserve(&mut self, device: &Device, capacity: usize) -> bool {
        if capacity <= self.capacity {
            return false;
        }
        self.capacity = capacity.next_power_of_two();
        self.buffer = storage_buffer::<T>(device, self.label, self.capacity);
        true
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }
}

fn storage_buffer<T: Pod>(device: &Device, label: &'static str, capacity: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (capacity * size_of::<T>()) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
