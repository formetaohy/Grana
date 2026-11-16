use bytemuck::cast_slice;
use grana_volume::CELL_COUNT;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

use crate::atlas::PACK_LIMIT;

pub(crate) const CELL_WORDS: usize = 1 + CELL_COUNT;

pub(crate) const STRIDE_WORDS: usize = CELL_WORDS * (1 + PACK_LIMIT);

pub(crate) struct Occupancy {
    buffer: Buffer,
    fold: Vec<u32>,
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
            size: (slots * STRIDE_WORDS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            fold: vec![0; slots * CELL_WORDS],
            slots,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    fn place(slot: usize) -> usize {
        slot * STRIDE_WORDS * size_of::<u32>()
    }

    pub fn reset(&mut self, slot: u32) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let start = slot as usize * CELL_WORDS;
        self.fold[start..start + CELL_WORDS].fill(0);
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
            "an occupancy pack holds one brick word and one word per cell"
        );
        let start = slot as usize * CELL_WORDS;
        for (folded, word) in self.fold[start..start + CELL_WORDS].iter_mut().zip(words) {
            if f32::from_bits(*word) > f32::from_bits(*folded) {
                *folded = *word;
            }
        }
        let offset = Self::place(slot as usize) + (1 + pack) * CELL_WORDS * size_of::<u32>();
        queue.write_buffer(&self.buffer, offset as u64, cast_slice(words));
    }

    pub fn seal(&self, queue: &Queue, slot: u32) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let start = slot as usize * CELL_WORDS;
        queue.write_buffer(
            &self.buffer,
            Self::place(slot as usize) as u64,
            cast_slice(&self.fold[start..start + CELL_WORDS]),
        );
    }
}
