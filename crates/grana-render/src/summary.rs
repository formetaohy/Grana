use bytemuck::cast_slice;
use grana_volume::{CELL_COUNT, CHANNEL_LIMIT};
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

pub(crate) const PACKS: usize = CHANNEL_LIMIT / 4;

pub(crate) const WORDS_PER_PLANE: usize = 2;

pub(crate) const WORDS_PER_SLOT: usize = CELL_COUNT * PACKS * WORDS_PER_PLANE;

const UNIT: f32 = 65535.0;

pub(crate) struct Summary {
    buffer: Buffer,
    slots: usize,
}

impl Summary {
    pub fn new(device: &Device, slots: usize) -> Self {
        assert!(slots > 0, "a summary covers at least one brick slot");
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("grana.brick.summary"),
            size: (slots * WORDS_PER_SLOT * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, slots }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn write(&self, queue: &Queue, slot: u32, cells: &[Vec<f32>]) {
        assert!(
            (slot as usize) < self.slots,
            "a summary cell block belongs to a brick slot"
        );
        assert!(
            cells.len() <= CHANNEL_LIMIT && cells.iter().all(|maxima| maxima.len() == CELL_COUNT),
            "a summary carries one cell maximum per channel"
        );
        let mut words = vec![0u32; WORDS_PER_SLOT];
        for (channel, maxima) in cells.iter().enumerate() {
            let plane = channel / 4;
            let component = channel % 4;
            for (cell, value) in maxima.iter().enumerate() {
                let base = (cell * PACKS + plane) * WORDS_PER_PLANE;
                let unit = (value.clamp(0.0, 1.0) * UNIT).round() as u32;
                words[base + component / 2] |= unit << (16 * (component % 2));
            }
        }
        queue.write_buffer(
            &self.buffer,
            slot as u64 * WORDS_PER_SLOT as u64 * size_of::<u32>() as u64,
            cast_slice(&words),
        );
    }
}
