use std::fs;
use std::io;
use std::path::Path;

use crate::canvas::Canvas;

const MACOS_CONTENT: f32 = 0.805;

pub fn write_ico(path: &Path, sizes: &[u32]) -> io::Result<()> {
    let images = sizes
        .iter()
        .map(|size| (*size, bitmap_entry(&grana_brand::icon(*size), *size)))
        .collect::<Vec<_>>();
    let mut file = Vec::new();
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dimension = u8::try_from(*size).unwrap_or(0);
        file.push(if *size >= 256 { 0 } else { dimension });
        file.push(if *size >= 256 { 0 } else { dimension });
        file.push(0);
        file.push(0);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        file.extend_from_slice(data);
    }
    fs::write(path, file)
}

pub fn write_icns(path: &Path, entries: &[(&[u8; 4], u32)]) -> io::Result<()> {
    let mut body = Vec::new();
    for (kind, size) in entries {
        let image = encode_png(&macos_icon(*size), *size, *size);
        body.extend_from_slice(*kind);
        body.extend_from_slice(&((image.len() + 8) as u32).to_be_bytes());
        body.extend_from_slice(&image);
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"icns");
    file.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    file.extend_from_slice(&body);
    fs::write(path, file)
}

fn macos_icon(size: u32) -> Vec<u8> {
    let content = (size as f32 * MACOS_CONTENT).round() as u32;
    let mark = grana_brand::icon(content);
    let mut canvas = Canvas::new(size, size);
    let offset = ((size - content) / 2) as i32;
    canvas.blit(&mark, content, (offset, offset));
    canvas.into_pixels()
}

fn encode_png(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the icon header encodes");
    writer
        .write_image_data(pixels)
        .expect("the icon pixels encode");
    drop(writer);
    output
}

fn bitmap_entry(pixels: &[u8], size: u32) -> Vec<u8> {
    let mask_stride = size.div_ceil(32) * 4;
    let mut data = Vec::with_capacity((40 + size * size * 4 + mask_stride * size) as usize);
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&(size as i32).to_le_bytes());
    data.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(size * size * 4 + mask_stride * size).to_le_bytes());
    data.extend_from_slice(&[0u8; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let index = ((y * size + x) * 4) as usize;
            data.extend_from_slice(&[
                pixels[index + 2],
                pixels[index + 1],
                pixels[index],
                pixels[index + 3],
            ]);
        }
    }
    for y in (0..size).rev() {
        let mut row = vec![0u8; mask_stride as usize];
        for x in 0..size {
            if pixels[((y * size + x) * 4 + 3) as usize] == 0 {
                row[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        data.extend_from_slice(&row);
    }
    data
}
