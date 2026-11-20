use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use grana_archive::{Block, Filler};
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
    raw: Vec<u8>,
    compressed: Vec<u8>,
}

impl Tile {
    pub(crate) fn new(sample: VoxelFormat) -> Self {
        Self {
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
        let extent = [
            SIDE.min(info.width - origin[0]),
            SIDE.min(info.height - origin[1]),
            1,
        ];
        let block = Block {
            level: request.level,
            frame: request.frame,
            channel: request.channel,
            origin,
            extent,
        };
        let data = fill(block)?;
        block.check(sample, &data);
        let data = data.cast(sample);
        let columns = extent[0] as usize;
        let rows = extent[1] as usize;
        let bytes = data.to_le_bytes();
        let width = columns * sample.bytes();
        let stride = SIDE as usize * sample.bytes();
        self.raw.fill(0);
        for line in 0..rows {
            let from = line * width;
            let to = line * stride;
            self.raw[to..to + width].copy_from_slice(&bytes[from..from + width]);
        }
        let mut compressed = std::mem::take(&mut self.compressed);
        compressed.clear();
        let mut encoder = ZlibEncoder::new(compressed, Compression::default());
        encoder
            .write_all(&self.raw)
            .map_err(|error| error.to_string())?;
        self.compressed = encoder.finish().map_err(|error| error.to_string())?;
        Ok(self.compressed.len() as u64)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.compressed
    }
}
