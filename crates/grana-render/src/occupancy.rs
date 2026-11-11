use grana_volume::CELL_COUNT;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

pub(crate) const OCCUPANCY_WORDS: usize = 1 + CELL_COUNT;

pub(crate) struct Occupancy {
    buffer: Buffer,
    slots: usize,
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
        Self { buffer, slots }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn write(&self, queue: &Queue, slot: u32, words: &[u32]) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        assert!(
            words.len() == OCCUPANCY_WORDS,
            "an occupancy slot holds one brick word and one word per cell"
        );
        queue.write_buffer(
            &self.buffer,
            slot as u64 * OCCUPANCY_WORDS as u64 * size_of::<u32>() as u64,
            bytemuck::cast_slice(words),
        );
    }
}
