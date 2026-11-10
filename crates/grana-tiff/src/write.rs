use std::path::Path;

use crate::Error;

const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_IMAGE_DESCRIPTION: u16 = 270;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_SOFTWARE: u16 = 305;
const TAG_SAMPLE_FORMAT: u16 = 339;

const SAMPLE_BITS: u16 = 16;
const UNCOMPRESSED: u16 = 1;
const BLACK_IS_ZERO: u16 = 1;
const ONE_SAMPLE: u16 = 1;
const UNSIGNED: u16 = 1;

const SOFTWARE: &str = "Grana";
const HEADER: u64 = 8;
const ENTRY: u64 = 12;
const PAGE_TAGS: u64 = 10;
const DESCRIBED_TAGS: u64 = PAGE_TAGS + 2;
const NAMES: [&str; 4] = ["Red", "Green", "Blue", "Alpha"];
const COLORS: [i32; 4] = [-65_536, -16_711_936, -16_776_961, -1];

pub struct Picture<'a> {
    pub size: [u32; 2],
    pub samples: u8,
    pub pixels: &'a [u16],
    pub name: &'a str,
}

pub fn write(path: &Path, picture: &Picture<'_>) -> Result<(), Error> {
    let bytes = encode(picture).map_err(|detail| Error::write(path, detail))?;
    grana_file::write(path, &bytes).map_err(|error| Error::write(path, error.to_string()))
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: u32,
}

impl Entry {
    fn long(tag: u16, value: u32) -> Self {
        Self {
            tag,
            kind: KIND_LONG,
            count: 1,
            value,
        }
    }

    fn short(tag: u16, value: u16) -> Self {
        Self {
            tag,
            kind: KIND_SHORT,
            count: 1,
            value: u32::from(value),
        }
    }

    fn text(tag: u16, count: u32, offset: u32) -> Self {
        Self {
            tag,
            kind: KIND_ASCII,
            count,
            value: offset,
        }
    }
}

fn encode(picture: &Picture<'_>) -> Result<Vec<u8>, String> {
    let [width, height] = picture.size;
    if width == 0 || height == 0 {
        return Err("a sixteen bit TIFF picture covers at least one pixel".to_string());
    }
    let samples = usize::from(picture.samples);
    if !(3..=4).contains(&samples) {
        return Err(format!(
            "a sixteen bit TIFF picture holds three or four samples per pixel, not {samples}"
        ));
    }
    let texels = width as usize * height as usize * samples;
    if picture.pixels.len() != texels {
        return Err(format!(
            "a sixteen bit TIFF picture of {width} by {height} with {samples} samples holds {texels} texels and was given {}",
            picture.pixels.len()
        ));
    }
    let pages = u64::from(picture.samples);
    let description = metadata(picture.name, width, height, samples);
    let described = page_bytes(DESCRIBED_TAGS);
    let plain = page_bytes(PAGE_TAGS);
    let first = HEADER;
    let description_at = first + described + (pages - 1) * plain;
    let software_at = description_at + description.len() as u64 + 1;
    let pixels_at = align(software_at + SOFTWARE.len() as u64 + 1, 2);
    let plane = u64::from(width) * u64::from(height) * 2;
    let end = pixels_at + plane * pages;
    if end > u64::from(u32::MAX) {
        return Err(
            "a sixteen bit TIFF picture exceeds the four gigabyte limit of the classic format"
                .to_string(),
        );
    }
    let description_at = u32::try_from(description_at).map_err(offset_fault)?;
    let software_at = u32::try_from(software_at).map_err(offset_fault)?;
    let mut bytes = Vec::with_capacity(end as usize);
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(first).map_err(offset_fault)?.to_le_bytes());
    for page in 0..pages {
        let next = if page + 1 == pages {
            0
        } else {
            u32::try_from(first + described + page * plain).map_err(offset_fault)?
        };
        let offset = u32::try_from(pixels_at + page * plane).map_err(offset_fault)?;
        let bytes_count = u32::try_from(plane).map_err(offset_fault)?;
        let mut entries = vec![
            Entry::long(TAG_IMAGE_WIDTH, width),
            Entry::long(TAG_IMAGE_LENGTH, height),
            Entry::short(TAG_BITS_PER_SAMPLE, SAMPLE_BITS),
            Entry::short(TAG_COMPRESSION, UNCOMPRESSED),
            Entry::short(TAG_PHOTOMETRIC, BLACK_IS_ZERO),
            Entry::long(TAG_STRIP_OFFSETS, offset),
            Entry::short(TAG_SAMPLES_PER_PIXEL, ONE_SAMPLE),
            Entry::long(TAG_ROWS_PER_STRIP, height),
            Entry::long(TAG_STRIP_BYTE_COUNTS, bytes_count),
            Entry::short(TAG_SAMPLE_FORMAT, UNSIGNED),
        ];
        if page == 0 {
            entries.push(Entry::text(
                TAG_IMAGE_DESCRIPTION,
                u32::try_from(description.len() + 1).map_err(offset_fault)?,
                description_at,
            ));
            entries.push(Entry::text(
                TAG_SOFTWARE,
                u32::try_from(SOFTWARE.len() + 1).map_err(offset_fault)?,
                software_at,
            ));
        }
        entries.sort_by_key(|entry| entry.tag);
        bytes.extend_from_slice(
            &u16::try_from(entries.len())
                .map_err(offset_fault)?
                .to_le_bytes(),
        );
        for entry in entries {
            bytes.extend_from_slice(&entry.tag.to_le_bytes());
            bytes.extend_from_slice(&entry.kind.to_le_bytes());
            bytes.extend_from_slice(&entry.count.to_le_bytes());
            bytes.extend_from_slice(&entry.value.to_le_bytes());
        }
        bytes.extend_from_slice(&next.to_le_bytes());
    }
    debug_assert_eq!(bytes.len() as u64, u64::from(description_at));
    bytes.extend_from_slice(description.as_bytes());
    bytes.push(0);
    debug_assert_eq!(bytes.len() as u64, u64::from(software_at));
    bytes.extend_from_slice(SOFTWARE.as_bytes());
    bytes.push(0);
    bytes.resize(pixels_at as usize, 0);
    for page in 0..samples {
        for pixel in picture.pixels.chunks_exact(samples) {
            bytes.extend_from_slice(&pixel[page].to_le_bytes());
        }
    }
    Ok(bytes)
}

fn page_bytes(tags: u64) -> u64 {
    2 + tags * ENTRY + 4
}

fn align(offset: u64, alignment: u64) -> u64 {
    offset.div_ceil(alignment) * alignment
}

fn offset_fault(_: impl std::fmt::Display) -> String {
    "a sixteen bit TIFF picture of this size has no addressable layout".to_string()
}

fn metadata(name: &str, width: u32, height: u32, samples: usize) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    xml.push_str(
        "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n <Image ID=\"Image:0\"",
    );
    xml.push_str(&format!(" Name=\"{}\">\n", escape(name)));
    xml.push_str(&format!(
        "  <Pixels ID=\"Pixels:0\" DimensionOrder=\"XYCZT\" Type=\"uint16\" SizeX=\"{width}\" SizeY=\"{height}\" SizeZ=\"1\" SizeC=\"{samples}\" SizeT=\"1\" Interleaved=\"false\">\n"
    ));
    for (index, (channel, color)) in NAMES.iter().zip(COLORS).take(samples).enumerate() {
        xml.push_str(&format!(
            "   <Channel ID=\"Channel:0:{index}\" Name=\"{channel}\" Color=\"{color}\" SamplesPerPixel=\"1\"/>\n"
        ));
    }
    xml.push_str(&format!(
        "   <TiffData IFD=\"0\" PlaneCount=\"{samples}\"/>\n  </Pixels>\n </Image>\n</OME>"
    ));
    xml
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}
