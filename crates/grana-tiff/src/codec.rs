use std::io::Read;

use flate2::read::{DeflateDecoder, ZlibDecoder};
use grana_volume::VoxelFormat;
use weezl::BitOrder;
use weezl::decode::Decoder as LzwDecoder;

const COMPRESSION_NONE: u16 = 1;
const COMPRESSION_LZW: u16 = 5;
const COMPRESSION_FAX3: u16 = 3;
const COMPRESSION_FAX4: u16 = 4;
const COMPRESSION_JPEG: u16 = 7;
const COMPRESSION_DEFLATE: u16 = 8;
const COMPRESSION_DEFLATE_LEGACY: u16 = 32946;
const COMPRESSION_PACKBITS: u16 = 32773;
const COMPRESSION_ZSTD: u16 = 50000;

const JPEG_START: [u8; 2] = [0xff, 0xd8];
const JPEG_END: [u8; 2] = [0xff, 0xd9];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compression {
    None,
    Lzw,
    Fax { group: u32 },
    Deflate,
    PackBits,
    Jpeg,
    Zstd,
    Other(u16),
}

impl Compression {
    pub(crate) fn of(code: u16) -> Self {
        match code {
            COMPRESSION_NONE => Self::None,
            COMPRESSION_LZW => Self::Lzw,
            COMPRESSION_FAX3 => Self::Other(COMPRESSION_FAX3),
            COMPRESSION_FAX4 => Self::Fax { group: 4 },
            COMPRESSION_JPEG => Self::Jpeg,
            COMPRESSION_DEFLATE | COMPRESSION_DEFLATE_LEGACY => Self::Deflate,
            COMPRESSION_PACKBITS => Self::PackBits,
            COMPRESSION_ZSTD => Self::Zstd,
            other => Self::Other(other),
        }
    }

    pub(crate) fn stores_rgb_samples(self) -> bool {
        self == Self::Jpeg
    }

    pub(crate) fn decode(self, input: &[u8], block: &Block<'_>) -> Result<Vec<u8>, String> {
        match self {
            Self::None => Ok(input.to_vec()),
            Self::Lzw => lzw(input),
            Self::Fax { group } => fax(input, block, group),
            Self::Deflate => deflate(input, block.expected),
            Self::PackBits => packbits(input, block.expected),
            Self::Jpeg => jpeg(input, block.tables),
            Self::Zstd => zstd(input),
            Self::Other(code) => Err(format!(
                "the TIFF compression {code} is not supported; Grana reads uncompressed, LZW, Deflate, PackBits, JPEG, ZSTD and CCITT Group 4 blocks"
            )),
        }
    }
}

fn lzw(input: &[u8]) -> Result<Vec<u8>, String> {
    LzwDecoder::with_tiff_size_switch(BitOrder::Msb, 8)
        .decode(input)
        .map_err(|error| format!("the TIFF LZW stream is invalid: {error}"))
}

fn deflate(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let limit = expected as u64 + 1;
    let mut decoded = Vec::new();
    if ZlibDecoder::new(input)
        .take(limit)
        .read_to_end(&mut decoded)
        .is_ok()
    {
        return Ok(decoded);
    }
    decoded.clear();
    DeflateDecoder::new(input)
        .take(limit)
        .read_to_end(&mut decoded)
        .map_err(|error| format!("the TIFF deflate stream is invalid: {error}"))?;
    Ok(decoded)
}

fn zstd(input: &[u8]) -> Result<Vec<u8>, String> {
    zstd::stream::decode_all(input)
        .map_err(|error| format!("the TIFF ZSTD stream is invalid: {error}"))
}

fn packbits(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::with_capacity(expected);
    let mut index = 0;
    while index < input.len() && decoded.len() < expected {
        let header = input[index] as i8;
        index += 1;
        if header >= 0 {
            let count = header as usize + 1;
            if index + count > input.len() {
                return Err("the TIFF PackBits stream ends inside a literal run".to_string());
            }
            decoded.extend_from_slice(&input[index..index + count]);
            index += count;
        } else if header != i8::MIN {
            let count = 1 - i32::from(header);
            let Some(value) = input.get(index) else {
                return Err("the TIFF PackBits stream ends inside a repeat run".to_string());
            };
            decoded.extend(std::iter::repeat_n(*value, count as usize));
            index += 1;
        }
    }
    Ok(decoded)
}

fn jpeg(input: &[u8], tables: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let stream = jpeg_stream(input, tables);
    let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(stream));
    decoder
        .decode()
        .map_err(|error| format!("the TIFF JPEG stream is invalid: {error}"))
}

fn jpeg_stream(input: &[u8], tables: Option<&[u8]>) -> Vec<u8> {
    let Some(tables) = tables else {
        return input.to_vec();
    };
    let mut stream = Vec::with_capacity(tables.len() + input.len());
    let tables = tables.strip_suffix(&JPEG_END).unwrap_or(tables);
    stream.extend_from_slice(tables);
    stream.extend_from_slice(input.strip_prefix(&JPEG_START).unwrap_or(input));
    stream
}

pub(crate) fn swap(format: VoxelFormat, data: &mut [u8]) {
    let width = format.bytes();
    if width == 1 {
        return;
    }
    for sample in data.chunks_exact_mut(width) {
        sample.reverse();
    }
}

pub(crate) fn reverse_predictor(
    format: VoxelFormat,
    data: &mut [u8],
    width: usize,
    rows: usize,
    samples: usize,
    floating: bool,
) -> Result<(), String> {
    let slots = width * samples;
    let size = format.bytes();
    if data.len() < slots * rows * size {
        return Err("the TIFF predictor meets a block shorter than its geometry".to_string());
    }
    if floating {
        return reverse_floating(format, data, slots, rows, samples);
    }
    match format {
        VoxelFormat::U8 => horizontal::<u8>(data, slots, rows, samples),
        VoxelFormat::I8 => horizontal::<i8>(data, slots, rows, samples),
        VoxelFormat::U16 => horizontal::<u16>(data, slots, rows, samples),
        VoxelFormat::I16 => horizontal::<i16>(data, slots, rows, samples),
        VoxelFormat::U32 => horizontal::<u32>(data, slots, rows, samples),
        VoxelFormat::U64 => horizontal::<u64>(data, slots, rows, samples),
        VoxelFormat::I32 => horizontal::<i32>(data, slots, rows, samples),
        VoxelFormat::I64 => horizontal::<i64>(data, slots, rows, samples),
        VoxelFormat::F32 => horizontal::<f32>(data, slots, rows, samples),
        VoxelFormat::F64 => horizontal::<f64>(data, slots, rows, samples),
    }
    Ok(())
}

trait Predicted: Copy {
    fn read(sample: &[u8]) -> Self;

    fn write(self, sample: &mut [u8]);

    fn added(self, other: Self) -> Self;
}

macro_rules! predicted {
    ($type:ty) => {
        impl Predicted for $type {
            fn read(sample: &[u8]) -> Self {
                <$type>::from_ne_bytes(sample.try_into().expect("a whole sample is read"))
            }

            fn write(self, sample: &mut [u8]) {
                sample.copy_from_slice(&self.to_ne_bytes());
            }

            fn added(self, other: Self) -> Self {
                self.wrapping_add(other)
            }
        }
    };
}

predicted!(u8);
predicted!(i8);
predicted!(u16);
predicted!(i16);
predicted!(u32);
predicted!(u64);
predicted!(i32);
predicted!(i64);

impl Predicted for f32 {
    fn read(sample: &[u8]) -> Self {
        Self::from_ne_bytes(sample.try_into().expect("a whole sample is read"))
    }

    fn write(self, sample: &mut [u8]) {
        sample.copy_from_slice(&self.to_ne_bytes());
    }

    fn added(self, other: Self) -> Self {
        f32::from_bits(self.to_bits().wrapping_add(other.to_bits()))
    }
}

impl Predicted for f64 {
    fn read(sample: &[u8]) -> Self {
        Self::from_ne_bytes(sample.try_into().expect("a whole sample is read"))
    }

    fn write(self, sample: &mut [u8]) {
        sample.copy_from_slice(&self.to_ne_bytes());
    }

    fn added(self, other: Self) -> Self {
        f64::from_bits(self.to_bits().wrapping_add(other.to_bits()))
    }
}

fn horizontal<T: Predicted>(data: &mut [u8], slots: usize, rows: usize, samples: usize) {
    let size = size_of::<T>();
    let mut values: Vec<T> = Vec::with_capacity(slots);
    for line in 0..rows {
        let start = line * slots * size;
        let row = &mut data[start..start + slots * size];
        values.clear();
        values.extend(row.chunks_exact(size).map(T::read));
        for index in samples..slots {
            values[index] = values[index].added(values[index - samples]);
        }
        for (sample, value) in row.chunks_exact_mut(size).zip(&values) {
            value.write(sample);
        }
    }
}

fn reverse_floating(
    format: VoxelFormat,
    data: &mut [u8],
    slots: usize,
    rows: usize,
    samples: usize,
) -> Result<(), String> {
    if matches!(format, VoxelFormat::U8 | VoxelFormat::I8) {
        return Err("the TIFF floating point predictor needs a wide sample".to_string());
    }
    let size = format.bytes();
    let row = slots * size;
    let mut native = vec![0u8; row];
    for line in 0..rows {
        let source = &mut data[line * row..(line + 1) * row];
        for plane in 0..size {
            let bytes = &mut source[plane * slots..(plane + 1) * slots];
            for index in samples..slots {
                bytes[index] = bytes[index].wrapping_add(bytes[index - samples]);
            }
        }
        for pixel in 0..slots {
            for plane in 0..size {
                native[pixel * size + plane] = source[plane * slots + pixel];
            }
        }
        if cfg!(target_endian = "little") {
            for sample in native.chunks_exact_mut(size) {
                sample.reverse();
            }
        }
        source.copy_from_slice(&native);
    }
    Ok(())
}

pub(crate) struct Block<'a> {
    pub expected: usize,
    pub width: u32,
    pub rows: u32,
    pub lsb_first: bool,
    pub tables: Option<&'a [u8]>,
}

fn fax(input: &[u8], block: &Block<'_>, group: u32) -> Result<Vec<u8>, String> {
    let width = block.width as usize;
    let rows = block.rows as usize;
    let row_bytes = width.div_ceil(8);
    let mut packed = vec![0u8; row_bytes * rows];
    let source: Vec<u8> = if block.lsb_first {
        input.iter().map(|byte| byte.reverse_bits()).collect()
    } else {
        input.to_vec()
    };
    let mut line = 0usize;
    let mut draw = |positions: &[u32]| {
        if line >= rows {
            return;
        }
        let base = line * row_bytes;
        let mut white = true;
        let mut next = 0usize;
        for x in 0..width {
            while next < positions.len() && positions[next] as usize <= x {
                white = !white;
                next += 1;
            }
            if !white {
                packed[base + x / 8] |= 1 << (7 - x % 8);
            }
        }
        line += 1;
    };
    let decoded = if group == 4 {
        fax::decoder::decode_g4(source.into_iter(), block.width, Some(block.rows), &mut draw)
    } else {
        fax::decoder::decode_g3(source.into_iter(), &mut draw)
    };
    if decoded.is_none() {
        return Err("the TIFF fax stream is invalid".to_string());
    }
    if line == 0 {
        return Err("the TIFF fax stream holds no line".to_string());
    }
    if packed.len() != block.expected {
        return Err("the TIFF fax stream does not cover its geometry".to_string());
    }
    Ok(packed)
}
