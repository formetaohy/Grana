use std::io::{BufWriter, Write};
use std::path::Path;

use crate::Error;

const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;
const KIND_LONG8: u16 = 16;

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
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_SOFTWARE: u16 = 305;
const TAG_EXTRA_SAMPLES: u16 = 338;
const TAG_SAMPLE_FORMAT: u16 = 339;

const UNCOMPRESSED: u16 = 1;
const RGB: u16 = 2;
const UNASSOCIATED_ALPHA: u16 = 2;
const CHUNKY: u16 = 1;
const UNSIGNED: u16 = 1;

const SOFTWARE: &str = "Grana";
const HEADER: u64 = 8;
const BIG_HEADER: u64 = 16;
const CLASSIC_LIMIT: u64 = u32::MAX as u64;
const NAMES: [&str; 4] = ["Red", "Green", "Blue", "Alpha"];
const COLORS: [i32; 4] = [-65_536, -16_711_936, -16_776_961, -1];

pub struct Picture<'a> {
    pub size: [u32; 2],
    pub samples: u8,
    pub pixels: &'a [u16],
    pub name: &'a str,
    pub physical: Option<[f32; 2]>,
    pub interval: Option<f32>,
}

pub struct Stream<'a> {
    pub size: [u32; 2],
    pub samples: u8,
    pub name: &'a str,
    pub physical: Option<[f32; 2]>,
    pub interval: Option<f32>,
}

pub struct Stack<'a> {
    pub size: [u32; 2],
    pub samples: u8,
    pub name: &'a str,
    pub physical: Option<[f32; 3]>,
    pub interval: Option<f32>,
    pub depth: u32,
}

pub type RowWriter<'a> = &'a mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>;

pub type LayerWriter<'a> = &'a mut dyn FnMut(u32, u32, &mut [u8]) -> Result<(), String>;

pub fn write(path: &Path, picture: &Picture<'_>) -> Result<(), Error> {
    let sheet = Sheet::new(
        picture.size,
        usize::from(picture.samples),
        16,
        1,
        picture.name,
        picture.physical.map(|[x, y]| [x, y, 0.0]),
        picture.interval,
    )
    .map_err(|detail| Error::write(path, detail))?;
    let texels = sheet.width as usize * sheet.height as usize * sheet.samples;
    if picture.pixels.len() != texels {
        return Err(Error::write(
            path,
            format!(
                "a sixteen bit picture of {} by {} with {} samples holds {texels} texels and was given {}",
                sheet.width,
                sheet.height,
                sheet.samples,
                picture.pixels.len()
            ),
        ));
    }
    write_sheet(path, &sheet, &mut |_, row, bytes| {
        let start = row as usize * sheet.width as usize * sheet.samples;
        let end = start + sheet.width as usize * sheet.samples;
        for (slot, sample) in bytes
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&picture.pixels[start..end])
        {
            slot.copy_from_slice(&sample.to_le_bytes());
        }
        Ok(())
    })
}

pub fn write_stream(path: &Path, stream: &Stream<'_>, row: RowWriter<'_>) -> Result<(), Error> {
    let sheet = Sheet::new(
        stream.size,
        usize::from(stream.samples),
        8,
        1,
        stream.name,
        stream.physical.map(|[x, y]| [x, y, 0.0]),
        stream.interval,
    )
    .map_err(|detail| Error::write(path, detail))?;
    write_sheet(path, &sheet, &mut |_, line, bytes| row(line, bytes))
}

pub fn write_stack(path: &Path, stack: &Stack<'_>, row: LayerWriter<'_>) -> Result<(), Error> {
    let sheet = Sheet::new(
        stack.size,
        usize::from(stack.samples),
        8,
        stack.depth,
        stack.name,
        stack.physical,
        stack.interval,
    )
    .map_err(|detail| Error::write(path, detail))?;
    write_sheet(path, &sheet, &mut |layer, line, bytes| {
        row(layer, line, bytes)
    })
}

struct Sheet<'a> {
    width: u32,
    height: u32,
    samples: usize,
    bits: u16,
    depth: u32,
    name: &'a str,
    physical: Option<[f32; 3]>,
    interval: Option<f32>,
}

impl<'a> Sheet<'a> {
    fn new(
        size: [u32; 2],
        samples: usize,
        bits: u16,
        depth: u32,
        name: &'a str,
        physical: Option<[f32; 3]>,
        interval: Option<f32>,
    ) -> Result<Self, String> {
        let [width, height] = size;
        if width == 0 || height == 0 {
            return Err("a TIFF picture covers at least one pixel".to_string());
        }
        if !(3..=4).contains(&samples) {
            return Err(format!(
                "a TIFF picture holds three or four samples per pixel, not {samples}"
            ));
        }
        if bits != 8 && bits != 16 {
            return Err(format!(
                "a TIFF picture holds eight or sixteen bit samples, not {bits}"
            ));
        }
        if depth == 0 {
            return Err("a TIFF picture holds at least one plane".to_string());
        }
        if let Some(physical) = physical
            && physical
                .iter()
                .take(2)
                .any(|value| !(value.is_finite() && *value > 0.0))
        {
            return Err("a TIFF picture spans a positive physical pixel size".to_string());
        }
        if let Some(interval) = interval
            && !(interval.is_finite() && interval > 0.0)
        {
            return Err("a TIFF picture carries a positive frame interval".to_string());
        }
        Ok(Self {
            width,
            height,
            samples,
            bits,
            depth,
            name,
            physical,
            interval,
        })
    }

    fn plane(&self) -> u64 {
        u64::from(self.width)
            * u64::from(self.height)
            * self.samples as u64
            * u64::from(self.bits / 8)
    }

    fn plan(&self, big: bool) -> Result<Plan, String> {
        let description = self.description();
        let inline = inline_size(big);
        let mut pages = Vec::with_capacity(self.depth as usize);
        let mut cursor = if big { BIG_HEADER } else { HEADER };
        for page in 0..self.depth {
            let tags = tags(self.samples, page == 0);
            let (count_size, entry_size, next_size) = if big { (8, 20, 8) } else { (2, 12, 4) };
            let ifd_at = cursor;
            let ifd_bytes = count_size + tags * entry_size + next_size;
            let mut at = ifd_at + ifd_bytes as u64;
            let bits_at = (!fits(self.samples * 2, inline)).then(|| {
                let held = at;
                at += self.samples as u64 * 2;
                held
            });
            let format_at = bits_at.map(|_| {
                let held = at;
                at += self.samples as u64 * 2;
                held
            });
            let (description_at, software_at) = if page == 0 {
                let description_at = (!fits(description.len() + 1, inline)).then(|| {
                    let held = at;
                    at += description.len() as u64 + 1;
                    held
                });
                let software_at = (!fits(SOFTWARE.len() + 1, inline)).then(|| {
                    let held = at;
                    at += SOFTWARE.len() as u64 + 1;
                    held
                });
                (description_at, software_at)
            } else {
                (None, None)
            };
            let pixels_at = align(at, 2);
            let end = pixels_at + self.plane();
            if !big && (end > CLASSIC_LIMIT || pixels_at > CLASSIC_LIMIT) {
                return Err("the picture outgrows the classic TIFF layout".to_string());
            }
            pages.push(Page {
                ifd_at,
                bits_at,
                format_at,
                description_at,
                software_at,
                pixels_at,
            });
            cursor = end;
        }
        let head = if big {
            let mut head = Vec::with_capacity(BIG_HEADER as usize);
            head.extend_from_slice(b"II");
            head.extend_from_slice(&43u16.to_le_bytes());
            head.extend_from_slice(&8u16.to_le_bytes());
            head.extend_from_slice(&0u16.to_le_bytes());
            head.extend_from_slice(&pages[0].ifd_at.to_le_bytes());
            head
        } else {
            let mut head = Vec::with_capacity(HEADER as usize);
            head.extend_from_slice(b"II");
            head.extend_from_slice(&42u16.to_le_bytes());
            head.extend_from_slice(
                &u32::try_from(pages[0].ifd_at)
                    .map_err(|_| "the TIFF header lies past its first directory".to_string())?
                    .to_le_bytes(),
            );
            head
        };
        Ok(Plan {
            big,
            pages,
            description,
            head,
            bytes: cursor,
        })
    }
}

fn tags(samples: usize, first: bool) -> usize {
    let base = if first { 13 } else { 11 };
    base + usize::from(samples == 4)
}

fn inline_size(big: bool) -> usize {
    if big { 8 } else { 4 }
}

fn fits(bytes: usize, inline: usize) -> bool {
    bytes <= inline
}

struct Page {
    ifd_at: u64,
    bits_at: Option<u64>,
    format_at: Option<u64>,
    description_at: Option<u64>,
    software_at: Option<u64>,
    pixels_at: u64,
}

struct Plan {
    big: bool,
    pages: Vec<Page>,
    description: String,
    head: Vec<u8>,
    bytes: u64,
}

type CellWriter<'a> = &'a mut dyn FnMut(u32, u32, &mut [u8]) -> Result<(), String>;

fn write_sheet(path: &Path, sheet: &Sheet<'_>, fill: CellWriter<'_>) -> Result<(), Error> {
    let fault = |detail: String| Error::write(path, detail);
    let plan = match sheet.plan(false) {
        Ok(plan) => plan,
        Err(_) => sheet.plan(true).map_err(fault)?,
    };
    write_plan(path, sheet, &plan, fill)
}

fn write_plan(
    path: &Path,
    sheet: &Sheet<'_>,
    plan: &Plan,
    fill: CellWriter<'_>,
) -> Result<(), Error> {
    let fault = |detail: String| Error::write(path, detail);
    let atomic = grana_file::Atomic::file(path).map_err(|error| fault(error.to_string()))?;
    let file = std::fs::File::create(atomic.staging()).map_err(|error| fault(error.to_string()))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&plan.head)
        .map_err(|error| fault(error.to_string()))?;
    let mut written = plan.head.len() as u64;
    let row_bytes = sheet.width as usize * sheet.samples * (sheet.bits as usize / 8);
    let mut buffer = vec![0u8; row_bytes];
    for (index, page) in plan.pages.iter().enumerate() {
        let next = plan.pages.get(index + 1).map_or(0, |next| next.ifd_at);
        if written != page.ifd_at {
            return Err(fault(format!(
                "the TIFF directory lies at {} after {written} bytes",
                page.ifd_at
            )));
        }
        let directory = directory(sheet, plan, page, next);
        writer
            .write_all(&directory)
            .map_err(|error| fault(error.to_string()))?;
        written += directory.len() as u64;
        let gap = usize::try_from(page.pixels_at - written)
            .map_err(|_| fault("a TIFF page holds no addressable gap".to_string()))?;
        if gap > 0 {
            writer
                .write_all(&vec![0u8; gap])
                .map_err(|error| fault(error.to_string()))?;
        }
        written = page.pixels_at;
        for line in 0..sheet.height {
            fill(index as u32, line, &mut buffer).map_err(fault)?;
            writer
                .write_all(&buffer)
                .map_err(|error| fault(error.to_string()))?;
            written += buffer.len() as u64;
        }
    }
    if written != plan.bytes {
        return Err(fault(format!(
            "the picture wrote {written} bytes and declares {} over {} by {} with {} samples",
            plan.bytes, sheet.width, sheet.height, sheet.samples
        )));
    }
    writer
        .into_inner()
        .map_err(|error| fault(error.to_string()))?
        .sync_all()
        .map_err(|error| fault(error.to_string()))?;
    atomic.commit().map_err(|error| fault(error.to_string()))
}

fn directory(sheet: &Sheet<'_>, plan: &Plan, page: &Page, next: u64) -> Vec<u8> {
    let big = plan.big;
    let inline = inline_size(big);
    let mut entries: Vec<(u16, u16, u64, Vec<u8>)> = Vec::new();
    let scalar = |tag: u16, kind: u16, value: u64| {
        let width = match kind {
            KIND_SHORT => 2,
            KIND_LONG => 4,
            _ => 8,
        };
        (tag, kind, 1, value.to_le_bytes()[..width].to_vec())
    };
    entries.push(scalar(TAG_IMAGE_WIDTH, KIND_LONG, u64::from(sheet.width)));
    entries.push(scalar(TAG_IMAGE_LENGTH, KIND_LONG, u64::from(sheet.height)));
    let bits = short_array(sheet.bits, sheet.samples);
    entries.push((
        TAG_BITS_PER_SAMPLE,
        KIND_SHORT,
        sheet.samples as u64,
        place(bits, page.bits_at, inline),
    ));
    entries.push(scalar(TAG_COMPRESSION, KIND_SHORT, u64::from(UNCOMPRESSED)));
    entries.push(scalar(TAG_PHOTOMETRIC, KIND_SHORT, u64::from(RGB)));
    let offset_kind = if big { KIND_LONG8 } else { KIND_LONG };
    entries.push(scalar(TAG_STRIP_OFFSETS, offset_kind, page.pixels_at));
    entries.push(scalar(
        TAG_SAMPLES_PER_PIXEL,
        KIND_SHORT,
        sheet.samples as u64,
    ));
    entries.push(scalar(
        TAG_ROWS_PER_STRIP,
        KIND_LONG,
        u64::from(sheet.height),
    ));
    entries.push(scalar(TAG_STRIP_BYTE_COUNTS, offset_kind, sheet.plane()));
    entries.push(scalar(
        TAG_PLANAR_CONFIGURATION,
        KIND_SHORT,
        u64::from(CHUNKY),
    ));
    let formats = short_array(UNSIGNED, sheet.samples);
    entries.push((
        TAG_SAMPLE_FORMAT,
        KIND_SHORT,
        sheet.samples as u64,
        place(formats, page.format_at, inline),
    ));
    if let Some(at) = page.description_at {
        let mut text = plan.description.as_bytes().to_vec();
        text.push(0);
        entries.push((
            TAG_IMAGE_DESCRIPTION,
            KIND_ASCII,
            text.len() as u64,
            place(text, Some(at), inline),
        ));
    }
    if let Some(at) = page.software_at {
        let mut software = SOFTWARE.as_bytes().to_vec();
        software.push(0);
        entries.push((
            TAG_SOFTWARE,
            KIND_ASCII,
            software.len() as u64,
            place(software, Some(at), inline),
        ));
    } else if page.description_at.is_some() {
        let mut software = SOFTWARE.as_bytes().to_vec();
        software.push(0);
        entries.push((TAG_SOFTWARE, KIND_ASCII, software.len() as u64, software));
    }
    if sheet.samples == 4 {
        entries.push(scalar(
            TAG_EXTRA_SAMPLES,
            KIND_SHORT,
            u64::from(UNASSOCIATED_ALPHA),
        ));
    }
    entries.sort_by_key(|(tag, ..)| *tag);
    let mut block = Vec::new();
    if big {
        block.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    } else {
        block.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    }
    for (tag, kind, count, value) in &entries {
        block.extend_from_slice(&tag.to_le_bytes());
        block.extend_from_slice(&kind.to_le_bytes());
        if big {
            block.extend_from_slice(&count.to_le_bytes());
        } else {
            block.extend_from_slice(&(u32::try_from(*count).unwrap_or(u32::MAX)).to_le_bytes());
        }
        let mut field = value.clone();
        field.resize(inline, 0);
        block.extend_from_slice(&field);
    }
    if big {
        block.extend_from_slice(&next.to_le_bytes());
    } else {
        block.extend_from_slice(&u32::try_from(next).unwrap_or(u32::MAX).to_le_bytes());
    }
    if page.bits_at.is_some() {
        block.extend_from_slice(&short_array(sheet.bits, sheet.samples));
    }
    if page.format_at.is_some() {
        block.extend_from_slice(&short_array(UNSIGNED, sheet.samples));
    }
    if page.description_at.is_some() {
        block.extend_from_slice(plan.description.as_bytes());
        block.push(0);
    }
    if page.software_at.is_some() {
        block.extend_from_slice(SOFTWARE.as_bytes());
        block.push(0);
    }
    block
}

fn short_array(value: u16, count: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(count * 2);
    for _ in 0..count {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn place(bytes: Vec<u8>, offset: Option<u64>, inline: usize) -> Vec<u8> {
    if bytes.len() <= inline {
        let mut field = bytes;
        field.resize(inline, 0);
        return field;
    }
    let offset = offset.expect("a TIFF value larger than its field is written out of line");
    offset.to_le_bytes()[..inline].to_vec()
}

fn align(offset: u64, alignment: u64) -> u64 {
    offset.div_ceil(alignment) * alignment
}

impl Sheet<'_> {
    fn description(&self) -> String {
        let width = self.width;
        let height = self.height;
        let samples = self.samples;
        let bits = self.bits;
        let depth = self.depth;
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        xml.push_str(
            "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n <Image ID=\"Image:0\"",
        );
        xml.push_str(&format!(" Name=\"{}\">\n", escape(self.name)));
        let physical = self.physical.map_or_else(String::new, |[x, y, z]| {
            let slice = (z.is_finite() && z > 0.0)
                .then(|| format!(" PhysicalSizeZ=\"{}\" PhysicalSizeZUnit=\"µm\"", number(z)));
            format!(
                " PhysicalSizeX=\"{}\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"{}\" PhysicalSizeYUnit=\"µm\"{}",
                number(x),
                number(y),
                slice.unwrap_or_default()
            )
        });
        let interval = self.interval.map_or_else(String::new, |interval| {
            format!(
                " TimeIncrement=\"{}\" TimeIncrementUnit=\"s\"",
                number(interval)
            )
        });
        xml.push_str(&format!(
            "  <Pixels ID=\"Pixels:0\" DimensionOrder=\"XYCZT\" Type=\"uint{bits}\" SizeX=\"{width}\" SizeY=\"{height}\" SizeZ=\"{depth}\" SizeC=\"{samples}\" SizeT=\"1\" Interleaved=\"true\"{physical}{interval}>\n"
        ));
        for (index, (channel, color)) in NAMES.iter().zip(COLORS).take(samples).enumerate() {
            xml.push_str(&format!(
                "   <Channel ID=\"Channel:0:{index}\" Name=\"{channel}\" Color=\"{color}\" SamplesPerPixel=\"1\"/>\n"
            ));
        }
        if depth == 1 {
            xml.push_str("   <TiffData IFD=\"0\" PlaneCount=\"1\"/>\n");
        } else {
            for plane in 0..depth {
                xml.push_str(&format!(
                    "   <TiffData IFD=\"{plane}\" PlaneCount=\"1\" FirstZ=\"{plane}\" FirstC=\"0\" FirstT=\"0\"/>\n"
                ));
            }
        }
        xml.push_str("  </Pixels>\n </Image>\n</OME>");
        xml
    }
}

fn number(value: f32) -> String {
    value.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_big_tiff_stack() {
        let path = std::env::temp_dir().join(format!("grana-bigtiff-{}.tif", std::process::id()));
        let sheet = Sheet::new([3, 2], 3, 8, 2, "big", Some([0.1, 0.1, 0.2]), None)
            .expect("the big picture declares its layout");
        let plan = sheet.plan(true).expect("the big layout plans");
        assert!(plan.big);
        write_plan(&path, &sheet, &plan, &mut |layer, row, buffer| {
            for (index, byte) in buffer.iter_mut().enumerate() {
                *byte = (layer * 37 + row * 3 + index as u32) as u8;
            }
            Ok(())
        })
        .expect("the big picture writes");
        let image = crate::Image::open(&path).expect("the big picture opens");
        let pyramid = image.pyramid();
        assert_eq!(pyramid.channel_count(), 3);
        assert_eq!(pyramid.level(0).extent().z(), 2);
        let data = image
            .read_region(0, 0, 1, 0..3, 0..2, 1..2)
            .expect("the big plane reads");
        let grana_volume::VoxelData::U8(values) = data else {
            panic!("the big picture stores eight bit voxels");
        };
        for y in 0..2u32 {
            for x in 0..3u32 {
                let index = (x + 3 * y) as usize;
                assert_eq!(
                    values[index],
                    (37 + y * 3 + x * 3 + 1) as u8,
                    "texel {x},{y}"
                );
            }
        }
        let _ = std::fs::remove_file(&path);
    }
}
