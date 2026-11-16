use bytemuck::cast_slice;
use grana_volume::CELL_COUNT;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

use crate::atlas::PACK_LIMIT;

pub(crate) const CELL_WORDS: usize = 1 + CELL_COUNT;

pub(crate) const COLUMN_WORDS: usize = CELL_WORDS * PACK_LIMIT;

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
            size: (slots * COLUMN_WORDS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, slots }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn write(&self, queue: &Queue, slot: u32, pack: usize, words: &[u32]) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        assert!(
            pack < PACK_LIMIT,
            "an occupancy pack names one of the channel packs"
        );
        assert!(
            words.len() == CELL_WORDS,
            "an occupancy pack holds one brick word and one word per cell"
        );
        let offset = (slot as usize * COLUMN_WORDS + pack * CELL_WORDS) * size_of::<u32>();
        queue.write_buffer(&self.buffer, offset as u64, cast_slice(words));
    }
}
