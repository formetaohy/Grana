use std::path::Path;
use std::sync::Arc;

use grana_volume::{Placement, VoxelData, VoxelFormat};

use crate::Cache;
use crate::codec::{Block as CodecBlock, Compression, reverse_predictor, swap};
use crate::color::{ColorModel, ColorValues};
use crate::directory::Directory;
use crate::error::Error;
use crate::layout::{Block, Layout};
use crate::sample::SampleType;
use crate::store::Store;
use crate::tag::{
    TAG_BITS_PER_SAMPLE, TAG_COLOR_MAP, TAG_COMPRESSION, TAG_EXTRA_SAMPLES, TAG_FILL_ORDER,
    TAG_IMAGE_LENGTH, TAG_IMAGE_WIDTH, TAG_JPEG_TABLES, TAG_NEW_SUBFILE_TYPE, TAG_PHOTOMETRIC,
    TAG_PLANAR_CONFIGURATION, TAG_PREDICTOR, TAG_SAMPLE_FORMAT, TAG_SAMPLES_PER_PIXEL,
    TAG_SUB_IFDS, TAG_TILE_LENGTH, TAG_TILE_WIDTH, TAG_YCBCR_COEFFICIENTS, TAG_YCBCR_SUBSAMPLING,
};

const PREDICTOR_NONE: u64 = 1;
const PREDICTOR_HORIZONTAL: u64 = 2;
const PREDICTOR_FLOATING: u64 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Predictor {
    None,
    Horizontal,
    Floating,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FillOrder {
    Msb,
    Lsb,
}

pub(crate) struct Page {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub planar: bool,
    pub tiled: bool,
    pub subfile: u64,
    pub sample_type: SampleType,
    components: u32,
    pub color: ColorModel,
    pub compression: Compression,
    pub sub_ifds: Vec<u64>,
    format: VoxelFormat,
    predictor: Predictor,
    fill_order: FillOrder,
    big_endian: bool,
    tables: Option<Vec<u8>>,
}

pub(crate) struct Source<'a> {
    pub store: &'a Store,
    pub cache: &'a Cache,
    pub path: &'a Path,
    pub file: usize,
}

pub(crate) struct Plane<'a> {
    pub layout: &'a Layout,
    pub source: &'a Source<'a>,
    pub level: usize,
    pub page: usize,
    pub channel: u32,
    pub chunky: bool,
    pub x: (u32, u32),
    pub y: (u32, u32),
}

impl Page {
    pub(crate) fn new(
        directory: &Directory,
        store: &Store,
        big_endian: bool,
        path: &Path,
    ) -> Result<Self, Error> {
        let width = required(directory, store, TAG_IMAGE_WIDTH, path)?;
        let height = required(directory, store, TAG_IMAGE_LENGTH, path)?;
        let samples = directory
            .integer(store, TAG_SAMPLES_PER_PIXEL, path)?
            .unwrap_or(1);
        let samples = u32::try_from(samples)
            .ok()
            .filter(|samples| *samples > 0)
            .ok_or_else(|| Error::unsupported(path, "the TIFF sample count is out of range"))?;
        let bits = uniform(directory, store, TAG_BITS_PER_SAMPLE, 1, path)?;
        let format = uniform(directory, store, TAG_SAMPLE_FORMAT, 1, path)?;
        let sample_type = SampleType::of(
            u16::try_from(bits)
                .map_err(|_| Error::unsupported(path, "the TIFF sample width is out of range"))?,
            u16::try_from(format).map_err(|_| {
                Error::unsupported(path, "the TIFF sample format code is out of range")
            })?,
        )
        .map_err(|detail| Error::unsupported(path, detail))?;
        let container = sample_type
            .container()
            .map_err(|detail| Error::unsupported(path, detail))?;
        sample_type
            .check_storage(container)
            .map_err(|detail| Error::unsupported(path, detail))?;
        let compression = directory
            .integer(store, TAG_COMPRESSION, path)?
            .unwrap_or(1);
        let compression = u16::try_from(compression)
            .map_err(|_| Error::unsupported(path, "the TIFF compression code is out of range"))?;
        let compression = Compression::of(compression);
        if compression.stores_rgb_samples() && sample_type.bits != 8 {
            return Err(Error::unsupported(
                path,
                "a JPEG compressed TIFF page stores eight bits per sample",
            ));
        }
        let predictor = match directory
            .integer(store, TAG_PREDICTOR, path)?
            .unwrap_or(PREDICTOR_NONE)
        {
            PREDICTOR_NONE => Predictor::None,
            PREDICTOR_HORIZONTAL => Predictor::Horizontal,
            PREDICTOR_FLOATING => Predictor::Floating,
            other => {
                return Err(Error::unsupported(
                    path,
                    format!("the TIFF predictor {other} is not supported"),
                ));
            }
        };
        if predictor != Predictor::None && sample_type.packed() {
            return Err(Error::unsupported(
                path,
                "a TIFF predictor meets packed samples",
            ));
        }
        let fill_order = match directory.integer(store, TAG_FILL_ORDER, path)?.unwrap_or(1) {
            1 => FillOrder::Msb,
            2 => FillOrder::Lsb,
            other => {
                return Err(Error::unsupported(
                    path,
                    format!("the TIFF fill order {other} is not supported"),
                ));
            }
        };
        let planar = match directory
            .integer(store, TAG_PLANAR_CONFIGURATION, path)?
            .unwrap_or(1)
        {
            1 => false,
            2 => true,
            other => {
                return Err(Error::unsupported(
                    path,
                    format!("the TIFF planar configuration {other} is not supported"),
                ));
            }
        };
        let color_map = directory.integers(store, TAG_COLOR_MAP, path)?;
        let subsampling = directory.integers(store, TAG_YCBCR_SUBSAMPLING, path)?;
        let coefficients = directory.rationals(store, TAG_YCBCR_COEFFICIENTS, path)?;
        let extra = directory.integers(store, TAG_EXTRA_SAMPLES, path)?;
        let values = ColorValues {
            color_map: (!color_map.is_empty()).then_some(color_map),
            subsampling: match subsampling.as_slice() {
                [horizontal, vertical] => Some([
                    u32::try_from(*horizontal).unwrap_or(u32::MAX),
                    u32::try_from(*vertical).unwrap_or(u32::MAX),
                ]),
                _ => None,
            },
            coefficients: (!coefficients.is_empty()).then_some(coefficients),
            extra,
        };
        let photometric = directory.integer(store, TAG_PHOTOMETRIC, path)?;
        let color = ColorModel::of(
            photometric,
            samples,
            sample_type,
            &values,
            compression.stores_rgb_samples(),
        )
        .map_err(|detail| Error::unsupported(path, detail))?;
        if sample_type.components() > 1
            && (color.converts() || samples != 1 || color.channels() != 1)
        {
            return Err(Error::unsupported(
                path,
                "complex TIFF samples are read for single-sample grayscale pages only",
            ));
        }
        if planar && color.converts() {
            return Err(Error::unsupported(
                path,
                "a TIFF page with separate planes cannot carry a converted color space",
            ));
        }
        let sub_ifds = directory.integers(store, TAG_SUB_IFDS, path)?;
        let tiled = directory.integer(store, TAG_TILE_WIDTH, path)?.is_some()
            || directory.integer(store, TAG_TILE_LENGTH, path)?.is_some();
        let subfile = directory
            .integer(store, TAG_NEW_SUBFILE_TYPE, path)?
            .unwrap_or(0);
        let tables = directory
            .bytes(store, TAG_JPEG_TABLES, path)?
            .map(|bytes| crate::codec::tables(&bytes, path))
            .transpose()?;
        Ok(Self {
            width,
            height,
            samples,
            components: sample_type.components(),
            planar,
            tiled,
            subfile,
            sample_type,
            color,
            compression,
            sub_ifds,
            format: container,
            predictor,
            fill_order,
            big_endian,
            tables,
        })
    }

    pub(crate) fn format(&self) -> VoxelFormat {
        self.format
    }

    pub(crate) fn bytes(&self) -> u64 {
        (size_of::<Self>() + self.sub_ifds.capacity() * size_of::<u64>()) as u64
    }

    pub(crate) fn channels(&self) -> u32 {
        if self.color.converts() {
            self.color.channels()
        } else {
            self.color.channels() * self.components
        }
    }

    pub(crate) fn read_plane(&self, plane: &Plane<'_>, out: &mut VoxelData) -> Result<(), Error> {
        assert_eq!(
            out.format(),
            self.format,
            "a decoded plane carries the page format"
        );
        let sample = if self.planar { plane.channel } else { 0 };
        let region = [(plane.x.0, plane.x.1), (plane.y.0, plane.y.1)];
        for block in plane
            .layout
            .covering(&region, self.width, self.height, sample)
        {
            let left = block.origin.0.max(plane.x.0);
            let right = (block.origin.0 + block.width).min(plane.x.1);
            let top = block.origin.1.max(plane.y.0);
            let bottom = (block.origin.1 + block.rows).min(plane.y.1);
            if left >= right || top >= bottom {
                continue;
            }
            let held = self.block(plane, sample, &block)?;
            let pixels = block.width as usize * block.rows as usize;
            let channels = (held.voxel_count() / pixels) as u32;
            let index = if self.planar || !plane.chunky {
                0
            } else {
                plane.channel
            };
            if index >= channels {
                return Err(Error::metadata(
                    plane.source.path,
                    format!(
                        "the TIFF page holds {channels} channels while the series reads channel {index}"
                    ),
                ));
            }
            let width = right - left;
            let extracted = extract(
                &held,
                block.width as usize,
                channels as usize,
                index as usize,
                left - block.origin.0..right - block.origin.0,
                top - block.origin.1..bottom - block.origin.1,
            );
            out.blit(
                &extracted,
                Placement::new(width, [0, 0]),
                Placement::new(plane.x.1 - plane.x.0, [left - plane.x.0, top - plane.y.0]),
                width,
                bottom - top,
            );
        }
        if self.color.inverted() && self.components == 1 {
            self.invert(out);
        }
        Ok(())
    }

    fn block(
        &self,
        plane: &Plane<'_>,
        sample: u32,
        block: &Block,
    ) -> Result<Arc<VoxelData>, Error> {
        let key = (
            plane.source.file,
            plane.level,
            plane.page,
            sample,
            block.index,
        );
        if let Some(held) = plane.source.cache.fetch(key) {
            return Ok(held);
        }
        let data = Arc::new(self.decode(plane.source, block)?);
        plane.source.cache.insert(key, Arc::clone(&data));
        Ok(data)
    }

    fn decode(&self, source: &Source<'_>, block: &Block) -> Result<VoxelData, Error> {
        let path = source.path;
        let pixels = block.width as usize * block.rows as usize;
        let samples = if self.planar {
            1
        } else {
            self.samples as usize
        } * self.components as usize;
        if block.count == 0 {
            return Ok(self.missing(pixels, samples));
        }
        let raw = source.store.read(block.offset, block.count as usize)?;
        let expected = if self.sample_type.packed() {
            (block.width as usize * samples * self.sample_type.bits as usize).div_ceil(8)
                * block.rows as usize
        } else {
            pixels * samples * self.format.bytes()
        };
        let decoded = self
            .compression
            .decode(
                &raw,
                &CodecBlock {
                    expected,
                    width: block.width,
                    rows: block.rows,
                    lsb_first: self.fill_order == FillOrder::Lsb,
                    tables: self.tables.as_deref(),
                },
            )
            .map_err(|detail| Error::read(path, detail))?;
        if matches!(self.compression, Compression::Fax { .. }) && self.sample_type.bits != 1 {
            return Err(Error::unsupported(
                path,
                "a TIFF fax stream carries one bit per pixel",
            ));
        }
        if self.compression.stores_rgb_samples() {
            if pixels == 0 || !decoded.len().is_multiple_of(pixels) {
                return Err(Error::read(
                    path,
                    "the decoded TIFF block does not cover whole pixels",
                ));
            }
            let channels = decoded.len() / pixels;
            if channels == 0 {
                return Err(Error::read(path, "the JPEG block carries no sample"));
            }
            if decoded.len() < expected {
                return Err(Error::read(
                    path,
                    shortage(block, self.sample_type.bits, decoded.len(), expected),
                ));
            }
            return Ok(VoxelData::reinterpret(self.format, &decoded));
        }
        let mut data = if decoded.len() < expected {
            if !block.padding {
                return Err(Error::read(
                    path,
                    shortage(block, self.sample_type.bits, decoded.len(), expected),
                ));
            }
            let mut data = decoded;
            data.resize(expected, 0);
            data
        } else {
            let mut data = decoded;
            data.truncate(expected);
            data
        };
        if self.sample_type.packed() {
            return self.unpack(&data, block, samples, path);
        }
        if self.predictor == Predictor::Floating {
            reverse_predictor(
                self.format,
                &mut data,
                block.width as usize,
                block.rows as usize,
                samples,
                true,
            )
            .map_err(|detail| Error::unsupported(path, detail))?;
        } else {
            if self.big_endian {
                swap(self.format, &mut data);
            }
            if self.predictor == Predictor::Horizontal {
                reverse_predictor(
                    self.format,
                    &mut data,
                    block.width as usize,
                    block.rows as usize,
                    samples,
                    false,
                )
                .map_err(|detail| Error::unsupported(path, detail))?;
            }
        }
        let reinterpreted = VoxelData::reinterpret(self.format, &data);
        if self.color.converts() {
            Ok(self.color.convert(
                self.format,
                self.sample_type.maximum(),
                samples,
                &reinterpreted,
            ))
        } else {
            Ok(reinterpreted)
        }
    }

    fn unpack(
        &self,
        data: &[u8],
        block: &Block,
        samples: usize,
        path: &Path,
    ) -> Result<VoxelData, Error> {
        let bits = self.sample_type.bits;
        let width = block.width as usize;
        let rows = block.rows as usize;
        let slots = width * samples;
        let row_bytes = (slots * bits as usize).div_ceil(8);
        if data.len() < row_bytes * rows {
            return Err(Error::read(
                path,
                shortage(block, bits, data.len(), row_bytes * rows),
            ));
        }
        let mirrored = (self.fill_order == FillOrder::Lsb
            && !matches!(self.compression, Compression::Fax { .. }))
        .then(|| {
            data.iter()
                .map(|byte| byte.reverse_bits())
                .collect::<Vec<u8>>()
        });
        let data = mirrored.as_deref().unwrap_or(data);
        let mask = (1u32 << bits) - 1;
        let mut values = Vec::with_capacity(slots * rows);
        for row in 0..rows {
            let line = &data[row * row_bytes..(row + 1) * row_bytes];
            for slot in 0..slots {
                let offset = slot * bits as usize;
                let at = offset / 8;
                let shift = offset % 8;
                let window = u32::from(line[at]) << 16
                    | u32::from(line.get(at + 1).copied().unwrap_or(0)) << 8
                    | u32::from(line.get(at + 2).copied().unwrap_or(0));
                values.push(((window >> (24 - shift - bits as usize)) & mask) as u16);
            }
        }
        Ok(match self.format {
            VoxelFormat::U8 => VoxelData::U8(values.into_iter().map(|value| value as u8).collect()),
            VoxelFormat::U16 => VoxelData::U16(values),
            VoxelFormat::I8 => VoxelData::I8(
                values
                    .into_iter()
                    .map(|value| sign_extend(value, bits) as i8)
                    .collect(),
            ),
            VoxelFormat::I16 => VoxelData::I16(
                values
                    .into_iter()
                    .map(|value| sign_extend(value, bits))
                    .collect(),
            ),
            _ => {
                return Err(Error::unsupported(
                    path,
                    format!(
                        "a TIFF packing {bits} bits per sample finds no eight or sixteen bit container"
                    ),
                ));
            }
        })
    }

    fn missing(&self, pixels: usize, samples: usize) -> VoxelData {
        let count = if self.compression.stores_rgb_samples() {
            pixels * self.channels() as usize
        } else {
            pixels * samples
        };
        VoxelData::blank(self.format, count)
    }

    fn invert(&self, data: &mut VoxelData) {
        let maximum = self.sample_type.maximum();
        match data {
            VoxelData::U8(values) => {
                let maximum = maximum as u8;
                values
                    .iter_mut()
                    .for_each(|value| *value = maximum.saturating_sub(*value));
            }
            VoxelData::U16(values) => {
                let maximum = maximum as u16;
                values
                    .iter_mut()
                    .for_each(|value| *value = maximum.saturating_sub(*value));
            }
            VoxelData::U32(values) => {
                let maximum = maximum as u32;
                values
                    .iter_mut()
                    .for_each(|value| *value = maximum.saturating_sub(*value));
            }
            VoxelData::U64(values) => {
                let maximum = maximum as u64;
                values
                    .iter_mut()
                    .for_each(|value| *value = maximum.saturating_sub(*value));
            }
            VoxelData::I8(values) => values
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            VoxelData::I16(values) => values
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            VoxelData::I32(values) => values
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            VoxelData::I64(values) => values
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            VoxelData::F32(values) => values.iter_mut().for_each(|value| *value = 1.0 - *value),
            VoxelData::F64(values) => values.iter_mut().for_each(|value| *value = 1.0 - *value),
        }
    }
}

fn extract(
    data: &VoxelData,
    width: usize,
    channels: usize,
    index: usize,
    x: std::ops::Range<u32>,
    y: std::ops::Range<u32>,
) -> VoxelData {
    macro_rules! arms {
        ($($variant:ident),*) => {
            match data {
                $(VoxelData::$variant(values) => VoxelData::$variant(extract_values(
                    values, width, channels, index, &x, &y,
                )),)*
            }
        };
    }
    arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}

fn extract_values<T: Copy>(
    values: &[T],
    width: usize,
    channels: usize,
    index: usize,
    x: &std::ops::Range<u32>,
    y: &std::ops::Range<u32>,
) -> Vec<T> {
    assert!(
        index < channels,
        "the extracted sample belongs to the block"
    );
    assert!(
        values.len() >= width * channels * y.end as usize,
        "the extracted window lies inside the block"
    );
    let span = (x.end - x.start) as usize;
    let mut extracted = Vec::with_capacity(span * (y.end - y.start) as usize);
    for row in y.start as usize..y.end as usize {
        let start = (row * width + x.start as usize) * channels + index;
        for column in 0..span {
            extracted.push(values[start + column * channels]);
        }
    }
    extracted
}

fn sign_extend(value: u16, bits: u16) -> i16 {
    ((value << (16 - bits)) as i16) >> (16 - bits)
}

fn shortage(block: &Block, bits: u16, decoded: usize, expected: usize) -> String {
    format!(
        "a TIFF block of {}x{} pixels at {bits} bits per sample decodes to {decoded} bytes where its geometry needs {expected}",
        block.width, block.rows
    )
}

fn required(directory: &Directory, store: &Store, tag: u16, path: &Path) -> Result<u32, Error> {
    let value = directory
        .integer(store, tag, path)?
        .ok_or_else(|| Error::unsupported(path, "the TIFF tag is missing"))?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::unsupported(path, format!("the TIFF tag {tag} is out of range")))
}

fn uniform(
    directory: &Directory,
    store: &Store,
    tag: u16,
    fallback: u64,
    path: &Path,
) -> Result<u64, Error> {
    let values = directory.integers(store, tag, path)?;
    let first = values.first().copied().unwrap_or(fallback);
    if values.iter().any(|value| *value != first) {
        return Err(Error::unsupported(
            path,
            format!("the TIFF tag {tag} differs between samples"),
        ));
    }
    Ok(first)
}
