use grana_volume::CELL_COUNT;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

pub(crate) const OCCUPANCY_WORDS: usize = 1 + CELL_COUNT;

pub(crate) struct Occupancy {
    buffer: Buffer,
    slots: usize,
    zeros: Vec<u32>,
}

impl Occupancy {
    pub fn new(device: &Device, slots: usize) -> Self {
        assert!(
            slots > 0,
            "an occupancy table covers at least one brick slot"
        );
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("grana.brick.occupancy"),
            size: (slots * OCCUPANCY_WORDS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            slots,
            zeros: vec![0; slots * OCCUPANCY_WORDS],
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn clear(&mut self, queue: &Queue) {
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.zeros));
    }

    pub fn clear_slot(&mut self, queue: &Queue, slot: u32) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let offset = slot as usize * OCCUPANCY_WORDS;
        queue.write_buffer(
            &self.buffer,
            offset as u64 * size_of::<u32>() as u64,
            bytemuck::cast_slice(&self.zeros[..OCCUPANCY_WORDS]),
        );
    }
}
