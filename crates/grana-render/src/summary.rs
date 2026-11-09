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
    words: Vec<u32>,
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
        Self {
            buffer,
            slots,
            words: vec![0; WORDS_PER_SLOT],
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn write(&mut self, queue: &Queue, slot: u32, cells: &[f32]) {
        assert!(
            (slot as usize) < self.slots,
            "a summary cell block belongs to a brick slot"
        );
        assert!(
            !cells.is_empty() && cells.len() / CELL_COUNT <= CHANNEL_LIMIT,
            "a summary carries one cell maximum per channel"
        );
        assert_eq!(
            cells.len() % CELL_COUNT,
            0,
            "a summary cell block covers whole channels"
        );
        self.words.fill(0);
        let channels = cells.len() / CELL_COUNT;
        for channel in 0..channels {
            let plane = channel / 4;
            let component = channel % 4;
            let maxima = &cells[channel * CELL_COUNT..(channel + 1) * CELL_COUNT];
            for (cell, value) in maxima.iter().enumerate() {
                let base = (cell * PACKS + plane) * WORDS_PER_PLANE;
                let unit = (value.clamp(0.0, 1.0) * UNIT).round() as u32;
                self.words[base + component / 2] |= unit << (16 * (component % 2));
            }
        }
        queue.write_buffer(
            &self.buffer,
            slot as u64 * WORDS_PER_SLOT as u64 * size_of::<u32>() as u64,
            cast_slice(&self.words),
        );
    }
}
