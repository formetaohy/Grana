use std::path::Path;

use crate::Error;
use crate::binary::{i32_at, u32_at, u64_at};
use crate::codec;
use crate::directory::{Compression, PixelType};
use crate::store::Store;

const SUBBLOCK_ID: &[u8] = b"ZISRAWSUBBLOCK";
const HEADER: usize = 80;
const ENTRY: usize = 48;
const ENTRY_MINIMUM: u64 = 240;
const AXIS_LENGTH: u64 = 20;
const AXIS_HEADER: u64 = 32;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tile {
    pub(crate) position: u64,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth: u32,
    pub(crate) plane: u32,
    pub(crate) mosaic: i32,
    pub(crate) pixel: PixelType,
    pub(crate) compression: Compression,
}

pub(crate) fn samples(store: &Store, path: &Path, tile: &Tile) -> Result<Vec<u8>, Error> {
    let header = store.read(tile.position, HEADER)?;
    if !header.starts_with(SUBBLOCK_ID) {
        return Err(Error::read(
            path,
            format!("the subblock at {} is not a ZISRAWSUBBLOCK", tile.position),
        ));
    }
    let metadata =
        u32_at(&header, 32).ok_or_else(|| Error::read(path, "the subblock header is truncated"))?;
    let data_size = u64_at(&header, 40)
        .filter(|size| *size > 0)
        .ok_or_else(|| Error::read(path, "the subblock header holds no pixel data"))?;
    let axes = i32_at(&header, 76)
        .filter(|count| *count > 0)
        .ok_or_else(|| Error::read(path, "the subblock header has no axis count"))?;
    let pixel = i32_at(&header, ENTRY + 2)
        .ok_or_else(|| Error::read(path, "the subblock header is truncated"))?;
    let compression = i32_at(&header, ENTRY + 18)
        .ok_or_else(|| Error::read(path, "the subblock header is truncated"))?;
    if pixel != pixel_value(tile.pixel) || compression != compression_value(tile.compression) {
        return Err(Error::read(
            path,
            format!(
                "the subblock at {} disagrees with its directory entry",
                tile.position
            ),
        ));
    }
    let entry = AXIS_HEADER + u64::from(axes as u32) * AXIS_LENGTH;
    let payload = tile.position + ENTRY as u64 + ENTRY_MINIMUM.max(entry) + u64::from(metadata);
    let body = u64_at(&header, 24)
        .filter(|size| *size > 0)
        .or_else(|| u64_at(&header, 16))
        .ok_or_else(|| Error::read(path, "the subblock segment has no body"))?;
    if payload + data_size > tile.position + ENTRY as u64 + body {
        return Err(Error::read(
            path,
            format!("the subblock at {} overruns its segment", tile.position),
        ));
    }
    let input = store.read(payload, data_size as usize)?;
    codec::decompress(
        tile.compression,
        tile.pixel,
        &input,
        [tile.width, tile.height],
        tile.depth,
    )
    .map_err(|detail| Error::read(path, detail))
}

fn pixel_value(pixel: PixelType) -> i32 {
    match pixel {
        PixelType::Gray8 => 0,
        PixelType::Gray16 => 1,
        PixelType::Gray32Float => 2,
    }
}

fn compression_value(compression: Compression) -> i32 {
    match compression {
        Compression::None => 0,
        Compression::JpegXr => 4,
        Compression::Zstd0 => 5,
        Compression::Zstd1 => 6,
    }
}
