use bytemuck::cast_slice;
use grana_volume::CELL_COUNT;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

use crate::atlas::PACK_LIMIT;

pub(crate) const CELL_WORDS: usize = 1 + CELL_COUNT;

pub(crate) const SLOT_WORDS: usize = CELL_WORDS * PACK_LIMIT;

pub(crate) struct Occupancy {
    buffer: Buffer,
    slots: usize,
    words: Vec<u32>,
}

impl Occupancy {
    pub fn new(device: &Device, slots: usize) -> Self {
        assert!(
            slots > 0,
            "an occupancy table covers at least one brick slot"
        );
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("grana.brick.occupancy"),
            size: (slots * SLOT_WORDS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            slots,
            words: vec![0; slots * SLOT_WORDS],
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn write(&mut self, queue: &Queue, slot: u32, pack: usize, words: &[u32]) {
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
            "an occupancy slot holds one brick word and one word per cell"
        );
        let base = slot as usize * SLOT_WORDS;
        for (cell, word) in words.iter().enumerate() {
            self.words[base + cell * PACK_LIMIT + pack] = *word;
        }
        queue.write_buffer(
            &self.buffer,
            (base * size_of::<u32>()) as u64,
            cast_slice(&self.words[base..base + SLOT_WORDS]),
        );
    }
}
