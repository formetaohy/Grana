use std::io::Cursor;

use openreadout_jpegxr::SampleType;

use crate::directory::{Compression, PixelType};

const DECODE_LIMIT_FACTOR: usize = 4;
const ZSTD1_HEADER: usize = 3;

pub(crate) fn decompress(
    compression: Compression,
    pixel: PixelType,
    input: &[u8],
    size: [u32; 2],
    depth: u32,
) -> Result<Vec<u8>, String> {
    let planes = size[0] as usize * size[1] as usize * pixel.bytes();
    let expected = planes * depth as usize;
    match compression {
        Compression::None => Ok(fit(input.to_vec(), expected)),
        Compression::JpegXr => {
            if depth != 1 {
                return Err(
                    "a CZI JPEG XR subblock spanning several planes is not supported".to_string(),
                );
            }
            jpeg_xr(pixel, input, expected, size)
        }
        Compression::Zstd0 => zstd(input, expected),
        Compression::Zstd1 => zstd1(input, expected),
    }
}

fn jpeg_xr(
    pixel: PixelType,
    input: &[u8],
    expected: usize,
    size: [u32; 2],
) -> Result<Vec<u8>, String> {
    let limit = expected.saturating_mul(DECODE_LIMIT_FACTOR).max(1);
    let image = openreadout_jpegxr::decode(input, limit).map_err(|error| error.to_string())?;
    if image.channels != 1 {
        return Err(format!(
            "the CZI JPEG XR subblock holds {} samples per pixel",
            image.channels
        ));
    }
    let wanted = match pixel {
        PixelType::Gray8 => SampleType::U8,
        PixelType::Gray16 => SampleType::U16,
        PixelType::Gray32Float => SampleType::F32,
    };
    if image.sample != wanted {
        return Err(format!(
            "the CZI JPEG XR subblock holds {:?} samples for a {:?} plane",
            image.sample, pixel
        ));
    }
    let row = image.width as usize * pixel.bytes();
    if image.data.len() < row * image.height as usize {
        return Err("the CZI JPEG XR subblock is truncated".to_string());
    }
    Ok(place(
        &image.data,
        [image.width, image.height],
        size,
        pixel.bytes(),
    ))
}

fn zstd(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let data = zstd::stream::decode_all(Cursor::new(input)).map_err(|error| error.to_string())?;
    if data.is_empty() {
        return Err("the CZI zstd subblock decodes to nothing".to_string());
    }
    Ok(fit(data, expected))
}

fn zstd1(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let (header, packed) = zstd1_header(input)?;
    let data = zstd(&input[header..], expected)?;
    if packed {
        return unpack_hi_lo(&data, expected);
    }
    Ok(data)
}

fn zstd1_header(input: &[u8]) -> Result<(usize, bool), String> {
    match input.first() {
        Some(1) => Ok((1, false)),
        Some(3) => {
            if input.len() < ZSTD1_HEADER || input[1] != 1 {
                return Err("the CZI zstd subblock carries an invalid header".to_string());
            }
            Ok((ZSTD1_HEADER, input[2] & 1 == 1))
        }
        _ => Err("the CZI zstd subblock carries an invalid header".to_string()),
    }
}

fn unpack_hi_lo(data: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    if !expected.is_multiple_of(2) || data.len() < expected {
        return Err("the CZI hi-lo byte packing needs a whole number of samples".to_string());
    }
    let words = expected / 2;
    let (low, high) = data.split_at(words);
    let mut samples = Vec::with_capacity(expected);
    for index in 0..words {
        samples.push(low[index]);
        samples.push(high[index]);
    }
    Ok(samples)
}

fn fit(mut data: Vec<u8>, expected: usize) -> Vec<u8> {
    data.resize(expected, 0);
    data
}

fn place(source: &[u8], from: [u32; 2], to: [u32; 2], sample: usize) -> Vec<u8> {
    let width = from[0].min(to[0]) as usize;
    let height = from[1].min(to[1]) as usize;
    let source_row = from[0] as usize * sample;
    let target_row = to[0] as usize * sample;
    let row = width * sample;
    let mut out = vec![0u8; target_row * to[1] as usize];
    for line in 0..height {
        let source = &source[line * source_row..line * source_row + row];
        let target = line * target_row;
        out[target..target + row].copy_from_slice(source);
    }
    out
}
