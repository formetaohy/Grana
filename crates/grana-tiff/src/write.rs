use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

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
const ENTRY: u64 = 12;
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

pub type RowWriter<'a> = &'a mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>;

pub fn write(path: &Path, picture: &Picture<'_>) -> Result<(), Error> {
    let layout = Layout::new(
        picture.size,
        usize::from(picture.samples),
        16,
        picture.name,
        picture.physical,
        picture.interval,
    )
    .map_err(|detail| Error::write(path, detail))?;
    let texels = layout.width as usize * layout.height as usize * layout.samples;
    if picture.pixels.len() != texels {
        return Err(Error::write(
            path,
            format!(
                "a sixteen bit picture of {} by {} with {} samples holds {texels} texels and was given {}",
                layout.width,
                layout.height,
                layout.samples,
                picture.pixels.len()
            ),
        ));
    }
    let mut writer = Writer::start(path, &layout)?;
    let mut row = vec![0u8; layout.width as usize * layout.samples * 2];
    for line in 0..layout.height as usize {
        let start = line * layout.width as usize * layout.samples;
        let end = start + layout.width as usize * layout.samples;
        let (slots, remainder) = row.as_chunks_mut::<2>();
        debug_assert!(remainder.is_empty());
        for (slot, sample) in slots.iter_mut().zip(&picture.pixels[start..end]) {
            slot.copy_from_slice(&sample.to_le_bytes());
        }
        writer.row(&row)?;
    }
    writer.finish(&layout)
}

pub fn write_stream(path: &Path, stream: &Stream<'_>, row: RowWriter<'_>) -> Result<(), Error> {
    let layout = Layout::new(
        stream.size,
        usize::from(stream.samples),
        8,
        stream.name,
        stream.physical,
        stream.interval,
    )
    .map_err(|detail| Error::write(path, detail))?;
    let mut writer = Writer::start(path, &layout)?;
    let mut buffer = vec![0u8; layout.width as usize * layout.samples];
    for line in 0..layout.height {
        row(line, &mut buffer).map_err(|detail| Error::write(path, detail))?;
        writer.row(&buffer)?;
    }
    writer.finish(&layout)
}

struct Layout {
    width: u32,
    height: u32,
    samples: usize,
    head: Vec<u8>,
    bytes: u64,
}

impl Layout {
    fn new(
        size: [u32; 2],
        samples: usize,
        bits: u16,
        name: &str,
        physical: Option<[f32; 2]>,
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
        if let Some(physical) = physical
            && physical
                .iter()
                .any(|value| !(value.is_finite() && *value > 0.0))
        {
            return Err("a TIFF picture spans a positive physical pixel size".to_string());
        }
        if let Some(interval) = interval
            && !(interval.is_finite() && interval > 0.0)
        {
            return Err("a TIFF picture carries a positive frame interval".to_string());
        }
        let description = metadata(width, height, samples, bits, name, physical, interval);
        let tags = if samples == 4 { 14 } else { 13 };
        let bits_at = HEADER + 2 + tags * ENTRY + 4;
        let format_at = bits_at + samples as u64 * 2;
        let description_at = format_at + samples as u64 * 2;
        let software_at = description_at + description.len() as u64 + 1;
        let pixels_at = align(software_at + SOFTWARE.len() as u64 + 1, 2);
        let plane = u64::from(width) * u64::from(height) * samples as u64 * u64::from(bits / 8);
        let end = pixels_at + plane;
        if end > u64::from(u32::MAX) {
            return Err(
                "a TIFF picture of this size exceeds the four gigabyte limit of the classic format"
                    .to_string(),
            );
        }
        let mut head = Vec::with_capacity(pixels_at as usize);
        head.extend_from_slice(b"II");
        head.extend_from_slice(&42u16.to_le_bytes());
        head.extend_from_slice(&u32::try_from(HEADER).map_err(offset_fault)?.to_le_bytes());
        let offset = |value: u64| u32::try_from(value).map_err(offset_fault);
        let count = |value: u64| u32::try_from(value).map_err(offset_fault);
        let mut entries = vec![
            Entry::long(TAG_IMAGE_WIDTH, width),
            Entry::long(TAG_IMAGE_LENGTH, height),
            Entry::array(TAG_BITS_PER_SAMPLE, KIND_SHORT, samples, offset(bits_at)?),
            Entry::short(TAG_COMPRESSION, UNCOMPRESSED),
            Entry::short(TAG_PHOTOMETRIC, RGB),
            Entry::text(
                TAG_IMAGE_DESCRIPTION,
                count(description.len() as u64 + 1)?,
                offset(description_at)?,
            ),
            Entry::long(TAG_STRIP_OFFSETS, offset(pixels_at)?),
            Entry::short(TAG_SAMPLES_PER_PIXEL, samples as u16),
            Entry::long(TAG_ROWS_PER_STRIP, height),
            Entry::long(
                TAG_STRIP_BYTE_COUNTS,
                u32::try_from(plane).map_err(offset_fault)?,
            ),
            Entry::short(TAG_PLANAR_CONFIGURATION, CHUNKY),
            Entry::text(
                TAG_SOFTWARE,
                count(SOFTWARE.len() as u64 + 1)?,
                offset(software_at)?,
            ),
            Entry::array(TAG_SAMPLE_FORMAT, KIND_SHORT, samples, offset(format_at)?),
        ];
        if samples == 4 {
            entries.push(Entry::short(TAG_EXTRA_SAMPLES, UNASSOCIATED_ALPHA));
        }
        entries.sort_by_key(|entry| entry.tag);
        head.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries {
            head.extend_from_slice(&entry.tag.to_le_bytes());
            head.extend_from_slice(&entry.kind.to_le_bytes());
            head.extend_from_slice(&entry.count.to_le_bytes());
            head.extend_from_slice(&entry.value.to_le_bytes());
        }
        head.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..samples {
            head.extend_from_slice(&bits.to_le_bytes());
        }
        for _ in 0..samples {
            head.extend_from_slice(&UNSIGNED.to_le_bytes());
        }
        debug_assert_eq!(head.len() as u64, description_at);
        head.extend_from_slice(description.as_bytes());
        head.push(0);
        debug_assert_eq!(head.len() as u64, software_at);
        head.extend_from_slice(SOFTWARE.as_bytes());
        head.push(0);
        head.resize(pixels_at as usize, 0);
        Ok(Self {
            width,
            height,
            samples,
            head,
            bytes: end,
        })
    }
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

    fn array(tag: u16, kind: u16, count: usize, offset: u32) -> Self {
        Self {
            tag,
            kind,
            count: u32::try_from(count).expect("a TIFF picture holds a few samples"),
            value: offset,
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

struct Writer {
    path: PathBuf,
    atomic: grana_file::Atomic,
    file: BufWriter<std::fs::File>,
    written: u64,
}

impl Writer {
    fn start(path: &Path, layout: &Layout) -> Result<Self, Error> {
        let fault = |detail: String| Error::write(path, detail);
        let atomic = grana_file::Atomic::file(path).map_err(|error| fault(error.to_string()))?;
        let file =
            std::fs::File::create(atomic.staging()).map_err(|error| fault(error.to_string()))?;
        let mut file = BufWriter::new(file);
        file.write_all(&layout.head)
            .map_err(|error| fault(error.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            atomic,
            file,
            written: layout.head.len() as u64,
        })
    }

    fn row(&mut self, row: &[u8]) -> Result<(), Error> {
        self.file
            .write_all(row)
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        self.written += row.len() as u64;
        Ok(())
    }

    fn finish(self, layout: &Layout) -> Result<(), Error> {
        let Self {
            path,
            atomic,
            file,
            written,
        } = self;
        if written != layout.bytes {
            return Err(Error::write(
                &path,
                format!(
                    "the picture wrote {written} bytes and declares {} over {} by {} with {} samples",
                    layout.bytes, layout.width, layout.height, layout.samples
                ),
            ));
        }
        file.into_inner()
            .map_err(|error| Error::write(&path, error.to_string()))?
            .sync_all()
            .map_err(|error| Error::write(&path, error.to_string()))?;
        atomic
            .commit()
            .map_err(|error| Error::write(&path, error.to_string()))
    }
}

fn align(offset: u64, alignment: u64) -> u64 {
    offset.div_ceil(alignment) * alignment
}

fn offset_fault(_: impl std::fmt::Display) -> String {
    "a TIFF picture of this size has no addressable layout".to_string()
}

fn metadata(
    width: u32,
    height: u32,
    samples: usize,
    bits: u16,
    name: &str,
    physical: Option<[f32; 2]>,
    interval: Option<f32>,
) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    xml.push_str(
        "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n <Image ID=\"Image:0\"",
    );
    xml.push_str(&format!(" Name=\"{}\">\n", escape(name)));
    let physical = physical.map_or_else(String::new, |[x, y]| {
        format!(
            " PhysicalSizeX=\"{}\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"{}\" PhysicalSizeYUnit=\"µm\"",
            number(x),
            number(y)
        )
    });
    let interval = interval.map_or_else(String::new, |interval| {
        format!(
            " TimeIncrement=\"{}\" TimeIncrementUnit=\"s\"",
            number(interval)
        )
    });
    xml.push_str(&format!(
        "  <Pixels ID=\"Pixels:0\" DimensionOrder=\"XYCZT\" Type=\"uint{bits}\" SizeX=\"{width}\" SizeY=\"{height}\" SizeZ=\"1\" SizeC=\"{samples}\" SizeT=\"1\" Interleaved=\"true\"{physical}{interval}>\n"
    ));
    for (index, (channel, color)) in NAMES.iter().zip(COLORS).take(samples).enumerate() {
        xml.push_str(&format!(
            "   <Channel ID=\"Channel:0:{index}\" Name=\"{channel}\" Color=\"{color}\" SamplesPerPixel=\"1\"/>\n"
        ));
    }
    xml.push_str("   <TiffData IFD=\"0\" PlaneCount=\"1\"/>\n  </Pixels>\n </Image>\n</OME>");
    xml
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
