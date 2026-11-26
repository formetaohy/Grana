use bytemuck::cast_slice;
use grana_volume::{CELL_COUNT, PACK_LIMIT};
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device};

use crate::upload::Upload;

pub(crate) const CELL_WORDS: usize = 1 + CELL_COUNT;

pub(crate) const STRIDE_WORDS: usize = CELL_WORDS;

const VALUE_LIMIT: f32 = 65535.0;

const MASK_SHIFT: u32 = 16;

pub(crate) fn bytes(slots: usize) -> u64 {
    (slots * STRIDE_WORDS * size_of::<u32>()) as u64
}

pub(crate) struct Occupancy {
    buffer: Buffer,
    columns: Vec<u16>,
    words: Vec<u32>,
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
            size: bytes(slots),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            columns: vec![0; slots * PACK_LIMIT * CELL_WORDS],
            words: vec![0; STRIDE_WORDS],
            slots,
        }
    }

    pub fn bytes(&self) -> u64 {
        bytes(self.slots)
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn reset(&mut self, slot: u32) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let start = slot as usize * PACK_LIMIT * CELL_WORDS;
        self.columns[start..start + PACK_LIMIT * CELL_WORDS].fill(0);
    }

    pub fn columns(&self, slot: u32) -> &[u16] {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let start = slot as usize * PACK_LIMIT * CELL_WORDS;
        &self.columns[start..start + PACK_LIMIT * CELL_WORDS]
    }

    pub fn adopt(&mut self, slot: u32, columns: &[u16]) {
        assert_eq!(
            columns.len(),
            PACK_LIMIT * CELL_WORDS,
            "an adopted occupancy slot holds a value per cell and per pack"
        );
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let start = slot as usize * PACK_LIMIT * CELL_WORDS;
        self.columns[start..start + columns.len()].copy_from_slice(columns);
    }

    pub fn stage(&mut self, slot: u32, pack: usize, values: &[f32]) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        assert!(
            pack < PACK_LIMIT,
            "an occupancy pack names one of the channel packs"
        );
        assert!(
            values.len() == CELL_WORDS,
            "an occupancy pack holds one brick value and one value per cell"
        );
        let start = (slot as usize * PACK_LIMIT + pack) * CELL_WORDS;
        for (column, value) in self.columns[start..start + CELL_WORDS]
            .iter_mut()
            .zip(values)
        {
            *column = quantize(*value);
        }
    }

    pub fn flush(&mut self, upload: &mut Upload, slot: u32) {
        assert!(
            (slot as usize) < self.slots,
            "an occupancy slot lies inside the table"
        );
        let base = slot as usize * PACK_LIMIT * CELL_WORDS;
        for (word, occupied) in self.words.iter_mut().enumerate() {
            let mut value = 0u16;
            let mut packs = 0u32;
            for pack in 0..PACK_LIMIT {
                let column = self.columns[base + pack * CELL_WORDS + word];
                value = value.max(column);
                if column != 0 {
                    packs |= 1 << pack;
                }
            }
            *occupied = u32::from(value) | (packs << MASK_SHIFT);
        }
        upload.buffer(
            &self.buffer,
            (slot as usize * STRIDE_WORDS * size_of::<u32>()) as u64,
            cast_slice(&self.words),
        );
    }
}

fn quantize(value: f32) -> u16 {
    (value * VALUE_LIMIT).round().clamp(0.0, VALUE_LIMIT) as u16
}
