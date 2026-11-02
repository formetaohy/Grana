use std::path::Path;

use grana_volume::VoxelFormat;

use crate::Error;
use crate::binary::{i32_at, i64_at};
use crate::segment;
use crate::store::Store;

const DIRECTORY_ID: &[u8] = b"ZISRAWDIRECTORY";
const SCHEMA: &[u8] = b"DV";
const DIRECTORY_HEADER: usize = 128;
const ENTRY_HEADER: usize = 32;
const AXIS_LENGTH: usize = 20;
const AXIS_LIMIT: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dimension {
    X,
    Y,
    Z,
    C,
    T,
    Scene,
    Mosaic,
    Block,
    Other(u8),
}

impl Dimension {
    fn parse(code: u8) -> Self {
        match code.to_ascii_uppercase() {
            b'X' => Self::X,
            b'Y' => Self::Y,
            b'Z' => Self::Z,
            b'C' => Self::C,
            b'T' => Self::T,
            b'S' => Self::Scene,
            b'M' => Self::Mosaic,
            b'B' => Self::Block,
            other => Self::Other(other),
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::X => "X".to_string(),
            Self::Y => "Y".to_string(),
            Self::Z => "Z".to_string(),
            Self::C => "C".to_string(),
            Self::T => "T".to_string(),
            Self::Scene => "S".to_string(),
            Self::Mosaic => "M".to_string(),
            Self::Block => "B".to_string(),
            Self::Other(code) => char::from(code).to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelType {
    Gray8,
    Gray16,
    Gray32Float,
}

impl PixelType {
    fn parse(value: i32) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Gray8),
            1 => Ok(Self::Gray16),
            2 => Ok(Self::Gray32Float),
            3 => Err("the CZI pixel type Bgr24 is not supported".to_string()),
            4 => Err("the CZI pixel type Bgr48 is not supported".to_string()),
            8 => Err("the CZI pixel type Bgr96Float is not supported".to_string()),
            9 => Err("the CZI pixel type Bgra32 is not supported".to_string()),
            10 | 11 => Err("the CZI pixel type complex float is not supported".to_string()),
            12 => Err("the CZI pixel type Gray32 is not supported".to_string()),
            13 => Err("the CZI pixel type Gray64 is not supported".to_string()),
            other => Err(format!("the CZI pixel type {other} is unknown")),
        }
    }

    pub(crate) fn format(self) -> VoxelFormat {
        match self {
            Self::Gray8 => VoxelFormat::U8,
            Self::Gray16 => VoxelFormat::U16,
            Self::Gray32Float => VoxelFormat::F32,
        }
    }

    pub(crate) fn bytes(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::Gray16 => 2,
            Self::Gray32Float => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compression {
    None,
    JpegXr,
    Zstd0,
    Zstd1,
}

impl Compression {
    fn parse(value: i32) -> Result<Self, String> {
        match value {
            0 => Ok(Self::None),
            4 => Ok(Self::JpegXr),
            5 => Ok(Self::Zstd0),
            6 => Ok(Self::Zstd1),
            1 => Err("the CZI JPEG compression is not supported".to_string()),
            2 => Err("the CZI LZW compression is not supported".to_string()),
            3 => Err("the CZI lossless JPEG compression is not supported".to_string()),
            7 => Err("the CZI chunked compression is not supported".to_string()),
            other => Err(format!("the CZI compression {other} is unknown")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Axis {
    pub(crate) dimension: Dimension,
    pub(crate) start: i32,
    pub(crate) size: u32,
    pub(crate) stored: u32,
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) position: u64,
    pub(crate) pixel: PixelType,
    pub(crate) compression: Compression,
    pub(crate) file_part: i32,
    pub(crate) axes: Vec<Axis>,
}

impl Entry {
    fn axis(&self, dimension: Dimension) -> Option<&Axis> {
        self.axes.iter().find(|axis| axis.dimension == dimension)
    }

    pub(crate) fn start(&self, dimension: Dimension) -> i32 {
        self.axis(dimension).map_or(0, |axis| axis.start)
    }

    pub(crate) fn size(&self, dimension: Dimension) -> u32 {
        self.axis(dimension).map_or(1, |axis| axis.size)
    }

    pub(crate) fn stored(&self, dimension: Dimension) -> u32 {
        self.axis(dimension).map_or(1, |axis| {
            if axis.stored == 0 {
                axis.size
            } else {
                axis.stored
            }
        })
    }

    pub(crate) fn is_full_resolution(&self) -> bool {
        self.stored(Dimension::X) == self.size(Dimension::X)
            && self.stored(Dimension::Y) == self.size(Dimension::Y)
    }

    fn parse(data: &[u8], offset: usize, path: &Path) -> Result<(Self, usize), Error> {
        let schema = data
            .get(offset..offset + 2)
            .ok_or_else(|| Error::metadata(path, "a directory entry is truncated"))?;
        if schema != SCHEMA {
            return Err(Error::unsupported(
                path,
                "the CZI directory entry schema is not DV",
            ));
        }
        let pixel = i32_at(data, offset + 2)
            .ok_or_else(|| Error::metadata(path, "a directory entry is truncated"))?;
        let position = i64_at(data, offset + 6)
            .filter(|position| *position > 0)
            .ok_or_else(|| Error::metadata(path, "a directory entry has no file position"))?;
        let file_part = i32_at(data, offset + 14)
            .ok_or_else(|| Error::metadata(path, "a directory entry is truncated"))?;
        let compression = i32_at(data, offset + 18)
            .ok_or_else(|| Error::metadata(path, "a directory entry is truncated"))?;
        let count = i32_at(data, offset + 28)
            .filter(|count| *count > 0 && *count as usize <= AXIS_LIMIT)
            .ok_or_else(|| Error::metadata(path, "a directory entry has an invalid axis count"))?
            as usize;
        let length = ENTRY_HEADER + count * AXIS_LENGTH;
        let mut axes = Vec::with_capacity(count);
        for index in 0..count {
            axes.push(axis(
                data,
                offset + ENTRY_HEADER + index * AXIS_LENGTH,
                path,
            )?);
        }
        for dimension in [Dimension::X, Dimension::Y] {
            if !axes.iter().any(|axis| axis.dimension == dimension) {
                return Err(Error::metadata(
                    path,
                    format!("a CZI subblock carries no {} dimension", dimension.label()),
                ));
            }
        }
        let pixel = PixelType::parse(pixel).map_err(|detail| Error::unsupported(path, detail))?;
        let compression =
            Compression::parse(compression).map_err(|detail| Error::unsupported(path, detail))?;
        Ok((
            Self {
                position: position as u64,
                pixel,
                compression,
                file_part,
                axes,
            },
            length,
        ))
    }
}

fn axis(data: &[u8], offset: usize, path: &Path) -> Result<Axis, Error> {
    let code = data
        .get(offset)
        .copied()
        .ok_or_else(|| Error::metadata(path, "a directory entry axis is truncated"))?;
    let start = i32_at(data, offset + 4)
        .ok_or_else(|| Error::metadata(path, "a directory entry axis is truncated"))?;
    let size = i32_at(data, offset + 8)
        .filter(|size| *size > 0)
        .ok_or_else(|| Error::metadata(path, "a directory entry axis has an invalid size"))?;
    let stored = i32_at(data, offset + 16)
        .filter(|stored| *stored >= 0)
        .ok_or_else(|| {
            Error::metadata(path, "a directory entry axis has an invalid stored size")
        })?;
    Ok(Axis {
        dimension: Dimension::parse(code),
        start,
        size: size as u32,
        stored: stored as u32,
    })
}

pub(crate) fn read(store: &Store, position: u64, path: &Path) -> Result<Vec<Entry>, Error> {
    let segment = segment::read(store, position, path)?;
    if !segment.is(DIRECTORY_ID) {
        return Err(Error::metadata(
            path,
            "the file header does not point at a subblock directory",
        ));
    }
    let body = usize::try_from(segment.body())
        .ok()
        .filter(|body| *body >= DIRECTORY_HEADER)
        .ok_or_else(|| Error::metadata(path, "the subblock directory is truncated"))?;
    let data = store.read(segment.data_offset(), body)?;
    let count = i32_at(&data, 0)
        .filter(|count| *count >= 0)
        .ok_or_else(|| Error::metadata(path, "the subblock directory has an invalid entry count"))?
        as usize;
    let mut entries = Vec::with_capacity(count);
    let mut offset = DIRECTORY_HEADER;
    for _ in 0..count {
        let (entry, length) = Entry::parse(&data, offset, path)?;
        entries.push(entry);
        offset += length;
    }
    if offset > data.len() {
        return Err(Error::metadata(
            path,
            "the subblock directory runs past its segment",
        ));
    }
    if entries.is_empty() {
        return Err(Error::metadata(path, "the subblock directory is empty"));
    }
    Ok(entries)
}
