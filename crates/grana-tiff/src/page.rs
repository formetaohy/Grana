use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use grana_volume::VoxelFormat;

use crate::Cache;
use crate::codec;
use crate::error::Error;
use crate::store::Store;
use crate::tag::{
    Directory, TAG_BITS_PER_SAMPLE, TAG_COMPRESSION, TAG_IMAGE_LENGTH, TAG_IMAGE_WIDTH,
    TAG_PHOTOMETRIC, TAG_PLANAR_CONFIGURATION, TAG_PREDICTOR, TAG_ROWS_PER_STRIP,
    TAG_SAMPLE_FORMAT, TAG_SAMPLES_PER_PIXEL, TAG_STRIP_BYTE_COUNTS, TAG_STRIP_OFFSETS,
    TAG_SUB_IFDS, TAG_TILE_BYTE_COUNTS, TAG_TILE_LENGTH, TAG_TILE_OFFSETS, TAG_TILE_WIDTH,
};

const COMPRESSION_NONE: u16 = 1;
const PREDICTOR_NONE: u16 = 1;
const PREDICTOR_HORIZONTAL: u16 = 2;

#[derive(Clone)]
pub(crate) struct Page {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub format: VoxelFormat,
    pub planar: bool,
    pub sub_ifds: Vec<u64>,
    compression: u16,
    predictor: u16,
    big_endian: bool,
    layout: Layout,
}

#[derive(Clone)]
enum Layout {
    Tiles {
        width: u32,
        height: u32,
        offsets: Vec<u64>,
        counts: Vec<u64>,
    },
    Strips {
        rows: u32,
        offsets: Vec<u64>,
        counts: Vec<u64>,
    },
}

pub(crate) struct Region<'a> {
    pub x: &'a Range<u32>,
    pub y: &'a Range<u32>,
}

pub(crate) struct Source<'a> {
    pub store: &'a Store,
    pub cache: &'a Cache,
    pub path: &'a Path,
    pub file: usize,
}

struct Block {
    index: usize,
    origin: (u32, u32),
    width: u32,
    rows: u32,
    offset: u64,
    count: u64,
}

pub(crate) trait Sample: Copy {
    fn load(bytes: &[u8], index: usize) -> Self;
}

impl Sample for u8 {
    fn load(bytes: &[u8], index: usize) -> Self {
        bytes[index]
    }
}

impl Sample for u16 {
    fn load(bytes: &[u8], index: usize) -> Self {
        let start = index * 2;
        Self::from_ne_bytes(
            bytes[start..start + 2]
                .try_into()
                .expect("two bytes are read"),
        )
    }
}

impl Sample for f32 {
    fn load(bytes: &[u8], index: usize) -> Self {
        let start = index * 4;
        Self::from_ne_bytes(
            bytes[start..start + 4]
                .try_into()
                .expect("four bytes are read"),
        )
    }
}

impl Page {
    pub(crate) fn new(
        directory: &Directory,
        store: &Store,
        big_endian: bool,
        path: &Path,
    ) -> Result<Self, Error> {
        let width = required_integer(directory, store, TAG_IMAGE_WIDTH, path)?;
        let height = required_integer(directory, store, TAG_IMAGE_LENGTH, path)?;
        let samples = directory
            .integer(store, TAG_SAMPLES_PER_PIXEL, path)?
            .unwrap_or(1);
        let bits = directory.integers(store, TAG_BITS_PER_SAMPLE, path)?;
        let bits = bits.first().copied().unwrap_or(1);
        if directory
            .integers(store, TAG_BITS_PER_SAMPLE, path)?
            .iter()
            .any(|value| *value != bits)
        {
            return Err(Error::unsupported(
                path,
                "the TIFF page mixes different sample widths",
            ));
        }
        let sample_format = directory
            .integer(store, TAG_SAMPLE_FORMAT, path)?
            .unwrap_or(1);
        let format = format(bits, sample_format, path)?;
        let compression = directory
            .integer(store, TAG_COMPRESSION, path)?
            .unwrap_or(u64::from(COMPRESSION_NONE));
        let compression = u16::try_from(compression)
            .map_err(|_| Error::unsupported(path, "the TIFF compression code is out of range"))?;
        let predictor = directory
            .integer(store, TAG_PREDICTOR, path)?
            .unwrap_or(u64::from(PREDICTOR_NONE));
        if predictor != u64::from(PREDICTOR_NONE) && predictor != u64::from(PREDICTOR_HORIZONTAL) {
            return Err(Error::unsupported(
                path,
                format!("the TIFF predictor {predictor} is not supported"),
            ));
        }
        if predictor == u64::from(PREDICTOR_HORIZONTAL) && format == VoxelFormat::F32 {
            return Err(Error::unsupported(
                path,
                "the TIFF floating point predictor is not supported",
            ));
        }
        let photometric = directory
            .integer(store, TAG_PHOTOMETRIC, path)?
            .unwrap_or(1);
        if !matches!(photometric, 0..=2) {
            return Err(Error::unsupported(
                path,
                format!("the TIFF photometric interpretation {photometric} is not supported"),
            ));
        }
        let planar = match directory
            .integer(store, TAG_PLANAR_CONFIGURATION, path)?
            .unwrap_or(1)
        {
            1 => false,
            2 => {
                if samples != 1 {
                    return Err(Error::unsupported(
                        path,
                        "a TIFF page with separate planes must hold one sample",
                    ));
                }
                true
            }
            other => {
                return Err(Error::unsupported(
                    path,
                    format!("the TIFF planar configuration {other} is not supported"),
                ));
            }
        };
        let samples = u32::try_from(samples)
            .ok()
            .filter(|samples| *samples > 0)
            .ok_or_else(|| Error::unsupported(path, "the TIFF sample count is out of range"))?;
        let layout = layout(directory, store, width, height, path)?;
        let sub_ifds = directory.integers(store, TAG_SUB_IFDS, path)?;
        Ok(Self {
            width,
            height,
            samples,
            format,
            planar,
            sub_ifds,
            compression,
            predictor: predictor as u16,
            big_endian,
            layout,
        })
    }

    pub(crate) fn bytes_per_sample(&self) -> usize {
        match self.format {
            VoxelFormat::U8 => 1,
            VoxelFormat::U16 => 2,
            VoxelFormat::F32 => 4,
        }
    }

    pub(crate) fn read_plane<T: Sample>(
        &self,
        source: &Source<'_>,
        level: usize,
        page_index: usize,
        sample: u32,
        region: &Region<'_>,
        out: &mut [T],
    ) -> Result<(), Error> {
        let Region { x, y } = region;
        let columns = (x.end - x.start) as usize;
        let rows = (y.end - y.start) as usize;
        assert_eq!(
            out.len(),
            columns * rows,
            "the plane buffer matches the region"
        );
        let samples = self.samples as usize;
        match &self.layout {
            Layout::Tiles {
                width,
                height,
                offsets,
                counts,
            } => {
                let across = self.width.div_ceil(*width);
                for tile_y in y.start / height..=(y.end - 1) / height {
                    for tile_x in x.start / width..=(x.end - 1) / width {
                        let index = (tile_y * across + tile_x) as usize;
                        let origin = (tile_x * width, tile_y * height);
                        let block = Block {
                            index,
                            origin,
                            width: *width,
                            rows: (*height).min(self.height - origin.1),
                            offset: offsets[index],
                            count: counts[index],
                        };
                        let data = self.block(source, level, page_index, &block)?;
                        copy_rows(&data, &block, samples, sample, region, out);
                    }
                }
            }
            Layout::Strips {
                rows: strip_rows,
                offsets,
                counts,
            } => {
                for strip in y.start / strip_rows..=(y.end - 1) / strip_rows {
                    let index = strip as usize;
                    let origin = (0, strip * strip_rows);
                    let block = Block {
                        index,
                        origin,
                        width: self.width,
                        rows: (*strip_rows).min(self.height - origin.1),
                        offset: offsets[index],
                        count: counts[index],
                    };
                    let data = self.block(source, level, page_index, &block)?;
                    copy_rows(&data, &block, samples, sample, region, out);
                }
            }
        }
        Ok(())
    }

    fn block(
        &self,
        source: &Source<'_>,
        level: usize,
        page_index: usize,
        block: &Block,
    ) -> Result<Arc<Vec<u8>>, Error> {
        let path = source.path;
        let key = (source.file, level, page_index, block.index);
        if let Some(data) = source.cache.fetch(key) {
            return Ok(data);
        }
        let expected = block.width as usize
            * block.rows as usize
            * self.samples as usize
            * self.bytes_per_sample();
        if block.count == 0 || block.offset == 0 {
            return Ok(Arc::new(vec![0; expected]));
        }
        let length = usize::try_from(block.count)
            .map_err(|_| Error::read(path, "the TIFF block is too large"))?;
        let raw = source.store.read(block.offset, length)?;
        let mut data = codec::decompress(self.compression, &raw, expected)
            .map_err(|detail| Error::read(path, detail))?;
        if data.len() < expected {
            return Err(Error::read(
                path,
                "the decoded TIFF block is shorter than its geometry",
            ));
        }
        data.truncate(expected);
        codec::to_native(self.format, &mut data, self.big_endian);
        if self.predictor == PREDICTOR_HORIZONTAL && self.compression != COMPRESSION_NONE {
            codec::reverse_predictor(
                self.format,
                &mut data,
                block.width as usize * self.samples as usize,
                block.rows as usize,
                self.samples as usize,
            )
            .map_err(|detail| Error::unsupported(path, detail))?;
        }
        let data = Arc::new(data);
        source.cache.insert(key, Arc::clone(&data));
        Ok(data)
    }
}

fn copy_rows<T: Sample>(
    data: &[u8],
    block: &Block,
    samples: usize,
    sample: u32,
    region: &Region<'_>,
    out: &mut [T],
) {
    let columns = (region.x.end - region.x.start) as usize;
    let first = region.y.start.max(block.origin.1);
    let last = region.y.end.min(block.origin.1 + block.rows);
    let left = region.x.start.max(block.origin.0);
    let right = region.x.end.min(block.origin.0 + block.width);
    for row in first..last {
        let source_row = (row - block.origin.1) as usize * block.width as usize;
        for column in left..right {
            let source =
                (source_row + (column - block.origin.0) as usize) * samples + sample as usize;
            let target =
                (row - region.y.start) as usize * columns + (column - region.x.start) as usize;
            out[target] = T::load(data, source);
        }
    }
}

fn required_integer(
    directory: &Directory,
    store: &Store,
    tag: u16,
    path: &Path,
) -> Result<u32, Error> {
    let value = directory
        .integer(store, tag, path)?
        .ok_or_else(|| Error::unsupported(path, format!("the TIFF tag {tag} is missing")))?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::unsupported(path, format!("the TIFF tag {tag} is out of range")))
}

fn format(bits: u64, sample_format: u64, path: &Path) -> Result<VoxelFormat, Error> {
    match (bits, sample_format) {
        (8, 1) => Ok(VoxelFormat::U8),
        (16, 1) => Ok(VoxelFormat::U16),
        (32, 3) => Ok(VoxelFormat::F32),
        _ => Err(Error::unsupported(
            path,
            format!(
                "the TIFF voxel type with {bits} bits and sample format {sample_format} is not supported"
            ),
        )),
    }
}

fn layout(
    directory: &Directory,
    store: &Store,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<Layout, Error> {
    let tile_width = directory.integer(store, TAG_TILE_WIDTH, path)?;
    let tile_length = directory.integer(store, TAG_TILE_LENGTH, path)?;
    if tile_width.is_some() || tile_length.is_some() {
        let tile_width = u32::try_from(tile_width.ok_or_else(|| {
            Error::unsupported(path, "the TIFF page declares tiles without a tile width")
        })?)
        .ok()
        .filter(|width| *width > 0)
        .ok_or_else(|| Error::unsupported(path, "the TIFF tile width is out of range"))?;
        let tile_length = u32::try_from(tile_length.ok_or_else(|| {
            Error::unsupported(path, "the TIFF page declares tiles without a tile length")
        })?)
        .ok()
        .filter(|length| *length > 0)
        .ok_or_else(|| Error::unsupported(path, "the TIFF tile length is out of range"))?;
        let offsets = directory.integers(store, TAG_TILE_OFFSETS, path)?;
        let counts = directory.integers(store, TAG_TILE_BYTE_COUNTS, path)?;
        let expected = width.div_ceil(tile_width) as usize * height.div_ceil(tile_length) as usize;
        if offsets.len() != expected || counts.len() != expected {
            return Err(Error::read(
                path,
                format!(
                    "the TIFF page declares {expected} tiles but lists {} offsets and {} byte counts",
                    offsets.len(),
                    counts.len()
                ),
            ));
        }
        return Ok(Layout::Tiles {
            width: tile_width,
            height: tile_length,
            offsets,
            counts,
        });
    }
    let rows = directory
        .integer(store, TAG_ROWS_PER_STRIP, path)?
        .map(|rows| {
            u32::try_from(rows)
                .ok()
                .filter(|rows| *rows > 0)
                .ok_or_else(|| Error::unsupported(path, "the TIFF rows per strip is out of range"))
        })
        .transpose()?
        .unwrap_or(height);
    let offsets = directory.integers(store, TAG_STRIP_OFFSETS, path)?;
    let counts = directory.integers(store, TAG_STRIP_BYTE_COUNTS, path)?;
    let expected = height.div_ceil(rows) as usize;
    if offsets.len() != expected || counts.len() != expected {
        return Err(Error::read(
            path,
            format!(
                "the TIFF page declares {expected} strips but lists {} offsets and {} byte counts",
                offsets.len(),
                counts.len()
            ),
        ));
    }
    Ok(Layout::Strips {
        rows,
        offsets,
        counts,
    })
}
