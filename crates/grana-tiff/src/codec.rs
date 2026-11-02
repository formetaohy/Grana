use std::io::Read;

use flate2::read::{DeflateDecoder, ZlibDecoder};
use grana_volume::VoxelFormat;

const COMPRESSION_NONE: u16 = 1;
const COMPRESSION_LZW: u16 = 5;
const COMPRESSION_DEFLATE: u16 = 8;
const COMPRESSION_DEFLATE_LEGACY: u16 = 32946;
const COMPRESSION_PACKBITS: u16 = 32773;

const LZW_CLEAR: u16 = 256;
const LZW_END: u16 = 257;
const LZW_FIRST: u16 = 258;
const LZW_LIMIT: usize = 4096;

pub(crate) fn decompress(
    compression: u16,
    input: &[u8],
    expected: usize,
) -> Result<Vec<u8>, String> {
    match compression {
        COMPRESSION_NONE => Ok(input.to_vec()),
        COMPRESSION_LZW => lzw(input, expected),
        COMPRESSION_DEFLATE | COMPRESSION_DEFLATE_LEGACY => deflate(input, expected),
        COMPRESSION_PACKBITS => packbits(input, expected),
        _ => Err(format!(
            "the TIFF compression {compression} is not supported"
        )),
    }
}

pub(crate) fn to_native(format: VoxelFormat, data: &mut [u8], big_endian: bool) {
    if !big_endian {
        return;
    }
    let size = match format {
        VoxelFormat::U8 => return,
        VoxelFormat::U16 => 2,
        VoxelFormat::F32 => 4,
    };
    for chunk in data.chunks_exact_mut(size) {
        chunk.reverse();
    }
}

pub(crate) fn reverse_predictor(
    format: VoxelFormat,
    data: &mut [u8],
    width: usize,
    rows: usize,
    stride: usize,
) -> Result<(), String> {
    match format {
        VoxelFormat::U8 => {
            for row in 0..rows {
                let start = row * width;
                for index in stride..width {
                    data[start + index] =
                        data[start + index].wrapping_add(data[start + index - stride]);
                }
            }
            Ok(())
        }
        VoxelFormat::U16 => {
            for row in 0..rows {
                let start = row * width * 2;
                for index in stride..width {
                    let previous = start + (index - stride) * 2;
                    let current = start + index * 2;
                    let previous = u16::from_ne_bytes(
                        data[previous..previous + 2]
                            .try_into()
                            .expect("two bytes are read"),
                    );
                    let current = u16::from_ne_bytes(
                        data[current..current + 2]
                            .try_into()
                            .expect("two bytes are read"),
                    );
                    data[start + index * 2..start + index * 2 + 2]
                        .copy_from_slice(&current.wrapping_add(previous).to_ne_bytes());
                }
            }
            Ok(())
        }
        VoxelFormat::F32 => Err("the TIFF floating point predictor is not supported".to_string()),
    }
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

fn lzw(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut prefix = vec![0u16; LZW_LIMIT];
    let mut suffix = vec![0u8; LZW_LIMIT];
    for (value, byte) in suffix.iter_mut().take(256).enumerate() {
        *byte = value as u8;
    }
    let mut stack = vec![0u8; LZW_LIMIT + 1];
    let mut width = 9;
    let mut next = LZW_FIRST as usize;
    let mut previous: Option<u16> = None;
    let mut bits = Bits::new(input);
    let mut decoded = Vec::with_capacity(expected);
    while let Some(code) = bits.next(width) {
        if code == LZW_CLEAR {
            width = 9;
            next = LZW_FIRST as usize;
            previous = None;
            continue;
        }
        if code == LZW_END {
            break;
        }
        let mut size = 0;
        let mut cursor = code;
        if code as usize == next {
            let Some(previous) = previous else {
                return Err("the TIFF LZW stream starts with an extended code".to_string());
            };
            stack[size] = first_byte(previous, &prefix, &suffix);
            size += 1;
            cursor = previous;
        } else if code as usize > next {
            return Err(format!(
                "the TIFF LZW stream references the undefined code {code}"
            ));
        }
        loop {
            if cursor < 256 {
                stack[size] = cursor as u8;
                size += 1;
                break;
            }
            stack[size] = suffix[cursor as usize];
            size += 1;
            cursor = prefix[cursor as usize];
        }
        let first = stack[size - 1];
        for index in (0..size).rev() {
            decoded.push(stack[index]);
        }
        if let Some(previous) = previous
            && next < LZW_LIMIT
        {
            prefix[next] = previous;
            suffix[next] = first;
            next += 1;
        }
        previous = Some(code);
        width = match next {
            511 => 10,
            1023 => 11,
            2047 => 12,
            _ => width,
        };
        if decoded.len() >= expected {
            break;
        }
    }
    Ok(decoded)
}

fn first_byte(code: u16, prefix: &[u16], suffix: &[u8]) -> u8 {
    let mut code = code;
    while code >= 256 {
        code = prefix[code as usize];
    }
    suffix[code as usize]
}

struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn next(&mut self, width: u32) -> Option<u16> {
        if self.position + width as usize > self.data.len() * 8 {
            return None;
        }
        let mut value = 0u16;
        for _ in 0..width {
            let byte = self.data[self.position / 8];
            let bit = (byte >> (7 - (self.position % 8))) & 1;
            value = (value << 1) | u16::from(bit);
            self.position += 1;
        }
        Some(value)
    }
}
