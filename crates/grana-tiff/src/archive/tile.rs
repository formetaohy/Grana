use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use grana_archive::{Filler, Sample};
use grana_volume::Level;

pub(crate) const SIDE: u32 = 256;

const BLOCK: usize = (SIDE * SIDE) as usize;

const MAXIMUM_16: f32 = u16::MAX as f32;

const MAXIMUM_8: f32 = u8::MAX as f32;

#[derive(Clone, Copy)]
pub(crate) struct LevelInfo {
    width: u32,
    height: u32,
    depth: u32,
    across: u32,
    down: u32,
}

impl LevelInfo {
    pub(crate) fn new(level: Level) -> Self {
        let extent = level.extent();
        Self {
            width: extent.x(),
            height: extent.y(),
            depth: extent.z(),
            across: extent.x().div_ceil(SIDE),
            down: extent.y().div_ceil(SIDE),
        }
    }

    pub(crate) fn width(self) -> u32 {
        self.width
    }

    pub(crate) fn height(self) -> u32 {
        self.height
    }

    pub(crate) fn depth(self) -> u32 {
        self.depth
    }

    pub(crate) fn across(self) -> u32 {
        self.across
    }

    pub(crate) fn down(self) -> u32 {
        self.down
    }

    pub(crate) fn tiles(self) -> usize {
        self.across as usize * self.down as usize
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Request {
    pub(crate) level: usize,
    pub(crate) frame: u32,
    pub(crate) channel: usize,
    pub(crate) z: u32,
}

pub(crate) struct Tile {
    block: Vec<f32>,
    scratch: Vec<f32>,
    raw: Vec<u8>,
    compressed: Vec<u8>,
}

impl Tile {
    pub(crate) fn new(sample: Sample) -> Self {
        let texel = bytes(sample);
        Self {
            block: vec![0.0; BLOCK],
            scratch: vec![0.0; BLOCK],
            raw: vec![0; BLOCK * texel],
            compressed: Vec::new(),
        }
    }

    pub(crate) fn encode(
        &mut self,
        request: &Request,
        info: LevelInfo,
        column: u32,
        row: u32,
        sample: Sample,
        fill: &Filler<'_>,
    ) -> Result<u64, String> {
        let origin = [column * SIDE, row * SIDE, request.z];
        let length = [
            SIDE.min(info.width - origin[0]),
            SIDE.min(info.height - origin[1]),
            1,
        ];
        let columns = length[0] as usize;
        let rows = length[1] as usize;
        let count = columns * rows;
        fill(
            request.level,
            request.frame,
            request.channel,
            origin,
            length,
            &mut self.scratch[..count],
        )?;
        self.block.fill(0.0);
        let stride = SIDE as usize;
        for line in 0..rows {
            let from = line * columns;
            let to = line * stride;
            self.block[to..to + columns].copy_from_slice(&self.scratch[from..from + columns]);
        }
        self.encode_block(sample);
        let mut compressed = std::mem::take(&mut self.compressed);
        compressed.clear();
        let mut encoder = ZlibEncoder::new(compressed, Compression::default());
        encoder
            .write_all(&self.raw)
            .map_err(|error| error.to_string())?;
        self.compressed = encoder.finish().map_err(|error| error.to_string())?;
        Ok(self.compressed.len() as u64)
    }

    fn encode_block(&mut self, sample: Sample) {
        match sample {
            Sample::Float => {
                for (slot, value) in self.raw.as_chunks_mut::<4>().0.iter_mut().zip(&self.block) {
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
            Sample::Unsigned16 => {
                for (slot, value) in self.raw.as_chunks_mut::<2>().0.iter_mut().zip(&self.block) {
                    let unit = (value.clamp(0.0, 1.0) * MAXIMUM_16).round() as u16;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            Sample::Unsigned8 => {
                for (slot, value) in self.raw.iter_mut().zip(&self.block) {
                    *slot = (value.clamp(0.0, 1.0) * MAXIMUM_8).round() as u8;
                }
            }
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.compressed
    }
}

pub(crate) fn bytes(sample: Sample) -> usize {
    match sample {
        Sample::Float => size_of::<f32>(),
        Sample::Unsigned16 => size_of::<u16>(),
        Sample::Unsigned8 => size_of::<u8>(),
    }
}
