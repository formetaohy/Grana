use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use grana_archive::Filler;
use grana_volume::{Level, VoxelFormat};

pub(crate) const SIDE: u32 = 256;

const BLOCK: usize = (SIDE * SIDE) as usize;

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
    pub(crate) fn new(sample: VoxelFormat) -> Self {
        Self {
            block: vec![0.0; BLOCK],
            scratch: vec![0.0; BLOCK],
            raw: vec![0; BLOCK * sample.bytes()],
            compressed: Vec::new(),
        }
    }

    pub(crate) fn encode(
        &mut self,
        request: &Request,
        info: LevelInfo,
        column: u32,
        row: u32,
        sample: VoxelFormat,
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

    fn encode_block(&mut self, sample: VoxelFormat) {
        match sample {
            VoxelFormat::U8 => {
                for (slot, value) in self.raw.iter_mut().zip(&self.block) {
                    *slot = value.round().clamp(0.0, f32::from(u8::MAX)) as u8;
                }
            }
            VoxelFormat::U16 => {
                for (slot, value) in self.raw.as_chunks_mut::<2>().0.iter_mut().zip(&self.block) {
                    let unit = value.round().clamp(0.0, f32::from(u16::MAX)) as u16;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::U64 => {
                for (slot, value) in self.raw.as_chunks_mut::<8>().0.iter_mut().zip(&self.block) {
                    let unit = value.round().clamp(0.0, u64::MAX as f32) as u64;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::I64 => {
                for (slot, value) in self.raw.as_chunks_mut::<8>().0.iter_mut().zip(&self.block) {
                    let unit = value.round().clamp(i64::MIN as f32, i64::MAX as f32) as i64;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::U32 => {
                for (slot, value) in self.raw.as_chunks_mut::<4>().0.iter_mut().zip(&self.block) {
                    let unit = value.round().clamp(0.0, u32::MAX as f32) as u32;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::I8 => {
                for (slot, value) in self.raw.iter_mut().zip(&self.block) {
                    *slot = value.round().clamp(f32::from(i8::MIN), f32::from(i8::MAX)) as i8 as u8;
                }
            }
            VoxelFormat::I16 => {
                for (slot, value) in self.raw.as_chunks_mut::<2>().0.iter_mut().zip(&self.block) {
                    let unit = value
                        .round()
                        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                        as i16;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::I32 => {
                for (slot, value) in self.raw.as_chunks_mut::<4>().0.iter_mut().zip(&self.block) {
                    let unit = value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
                    slot.copy_from_slice(&unit.to_le_bytes());
                }
            }
            VoxelFormat::F32 => {
                for (slot, value) in self.raw.as_chunks_mut::<4>().0.iter_mut().zip(&self.block) {
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
            VoxelFormat::F64 => {
                for (slot, value) in self.raw.as_chunks_mut::<8>().0.iter_mut().zip(&self.block) {
                    slot.copy_from_slice(&f64::from(*value).to_le_bytes());
                }
            }
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.compressed
    }
}
