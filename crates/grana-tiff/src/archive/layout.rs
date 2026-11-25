use grana_archive::ImageSpec;
use grana_volume::{ColorTable, VoxelFormat};

use super::tile::SIDE;

const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;
const KIND_LONG8: u16 = 16;
const KIND_IFD8: u16 = 18;

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_IMAGE_DESCRIPTION: u16 = 270;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_SOFTWARE: u16 = 305;
const TAG_COLOR_MAP: u16 = 320;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SUB_IFDS: u16 = 330;
const TAG_SAMPLE_FORMAT: u16 = 339;

const DEFLATE: u16 = 8;
const BLACK_IS_ZERO: u16 = 1;
const PALETTE: u16 = 3;
const CHUNKY: u16 = 1;
const UNSIGNED: u16 = 1;
const FLOAT: u16 = 3;
const SIGNED: u16 = 2;

const SOFTWARE: &str = "Grana";
const COUNT_BYTES: usize = 8;
const ENTRY_BYTES: usize = 20;
const NEXT_BYTES: usize = 8;
const FIELD_BYTES: usize = 8;
const VALUE_BYTES: usize = 12;
const BLOCK_ALIGN: usize = 2;
const COLOR_MAP_ENTRIES: usize = 1 << 8;

pub(crate) struct Spots {
    pub(crate) next: u64,
    pub(crate) offsets: u64,
    pub(crate) counts: u64,
    pub(crate) sub_ifds: Option<u64>,
}

enum Field {
    Scalar(u64),
    Blob(Vec<u8>),
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u64,
    field: Field,
}

pub(crate) fn header() -> [u8; 16] {
    let mut head = [0u8; 16];
    head[..2].copy_from_slice(b"II");
    head[2..4].copy_from_slice(&43u16.to_le_bytes());
    head[4..6].copy_from_slice(&8u16.to_le_bytes());
    head[6..8].copy_from_slice(&0u16.to_le_bytes());
    head[8..].copy_from_slice(&16u64.to_le_bytes());
    head
}

#[derive(Clone, Copy)]
pub(crate) struct Sample<'a> {
    pub(crate) format: VoxelFormat,
    pub(crate) color_table: Option<&'a ColorTable>,
}

pub(crate) fn page(
    at: u64,
    width: u32,
    height: u32,
    tiles: usize,
    sub_levels: usize,
    sample: Sample<'_>,
    description: Option<&str>,
) -> (Vec<u8>, Spots) {
    block(
        at,
        entries(width, height, tiles, sub_levels, sample, description),
    )
}

pub(crate) fn sub_page(
    at: u64,
    width: u32,
    height: u32,
    tiles: usize,
    sample: Sample<'_>,
) -> (Vec<u8>, Spots) {
    block(at, entries(width, height, tiles, 0, sample, None))
}

fn entries(
    width: u32,
    height: u32,
    tiles: usize,
    sub_levels: usize,
    sample: Sample<'_>,
    description: Option<&str>,
) -> Vec<Entry> {
    let Sample {
        format,
        color_table,
    } = sample;
    let mut entries = Vec::with_capacity(16);
    entries.push(scalar(TAG_IMAGE_WIDTH, KIND_LONG, u64::from(width)));
    entries.push(scalar(TAG_IMAGE_LENGTH, KIND_LONG, u64::from(height)));
    entries.push(scalar(
        TAG_BITS_PER_SAMPLE,
        KIND_SHORT,
        u64::from(bits(format)),
    ));
    entries.push(scalar(TAG_COMPRESSION, KIND_SHORT, u64::from(DEFLATE)));
    entries.push(scalar(
        TAG_PHOTOMETRIC,
        KIND_SHORT,
        u64::from(match color_table {
            Some(_) => PALETTE,
            None => BLACK_IS_ZERO,
        }),
    ));
    if let Some(description) = description {
        entries.push(text(TAG_IMAGE_DESCRIPTION, description));
    }
    entries.push(scalar(TAG_SAMPLES_PER_PIXEL, KIND_SHORT, 1));
    entries.push(scalar(
        TAG_PLANAR_CONFIGURATION,
        KIND_SHORT,
        u64::from(CHUNKY),
    ));
    if description.is_some() {
        entries.push(text(TAG_SOFTWARE, SOFTWARE));
    }
    if let Some(color_table) = color_table {
        entries.push(color_map(color_table));
    }
    entries.push(scalar(TAG_TILE_WIDTH, KIND_LONG, u64::from(SIDE)));
    entries.push(scalar(TAG_TILE_LENGTH, KIND_LONG, u64::from(SIDE)));
    entries.push(blob(TAG_TILE_OFFSETS, KIND_LONG8, tiles * 8));
    entries.push(blob(TAG_TILE_BYTE_COUNTS, KIND_LONG8, tiles * 8));
    if sub_levels > 0 {
        entries.push(blob(TAG_SUB_IFDS, KIND_IFD8, sub_levels * 8));
    }
    entries.push(scalar(
        TAG_SAMPLE_FORMAT,
        KIND_SHORT,
        u64::from(sample_format(format)),
    ));
    entries
}

fn color_map(color_table: &ColorTable) -> Entry {
    let colors = color_table.colors();
    let mut bytes = Vec::with_capacity(COLOR_MAP_ENTRIES * 3 * 2);
    for component in 0..3 {
        for index in 0..COLOR_MAP_ENTRIES {
            let color = colors[index.min(colors.len() - 1)];
            let value = (color[component].clamp(0.0, 1.0) * 65535.0).round() as u16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Entry {
        tag: TAG_COLOR_MAP,
        kind: KIND_SHORT,
        count: (COLOR_MAP_ENTRIES * 3) as u64,
        field: Field::Blob(bytes),
    }
}

fn bits(sample: VoxelFormat) -> u16 {
    (sample.bytes() * 8) as u16
}

fn sample_format(sample: VoxelFormat) -> u16 {
    match sample {
        VoxelFormat::U8 | VoxelFormat::U16 | VoxelFormat::U32 | VoxelFormat::U64 => UNSIGNED,
        VoxelFormat::I8 | VoxelFormat::I16 | VoxelFormat::I32 | VoxelFormat::I64 => SIGNED,
        VoxelFormat::F32 | VoxelFormat::F64 => FLOAT,
    }
}

fn scalar(tag: u16, kind: u16, value: u64) -> Entry {
    Entry {
        tag,
        kind,
        count: 1,
        field: Field::Scalar(value),
    }
}

fn blob(tag: u16, kind: u16, bytes: usize) -> Entry {
    Entry {
        tag,
        kind,
        count: bytes as u64 / width(kind),
        field: Field::Blob(vec![0; bytes]),
    }
}

fn text(tag: u16, value: &str) -> Entry {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    Entry {
        tag,
        kind: KIND_ASCII,
        count: bytes.len() as u64,
        field: Field::Blob(bytes),
    }
}

fn width(kind: u16) -> u64 {
    match kind {
        KIND_SHORT => 2,
        KIND_LONG => 4,
        KIND_LONG8 | KIND_IFD8 => 8,
        _ => 1,
    }
}

fn block(at: u64, entries: Vec<Entry>) -> (Vec<u8>, Spots) {
    let head = COUNT_BYTES + entries.len() * ENTRY_BYTES + NEXT_BYTES;
    let next = at + (COUNT_BYTES + entries.len() * ENTRY_BYTES) as u64;
    let payload_start = at + head as u64;
    let mut spots = Spots {
        next,
        offsets: 0,
        counts: 0,
        sub_ifds: None,
    };
    let mut fields = vec![0u8; entries.len() * FIELD_BYTES];
    let mut payload: Vec<u8> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let field = &mut fields[index * FIELD_BYTES..(index + 1) * FIELD_BYTES];
        let field_at = at + (COUNT_BYTES + index * ENTRY_BYTES + VALUE_BYTES) as u64;
        match &entry.field {
            Field::Scalar(value) => field.copy_from_slice(&value.to_le_bytes()),
            Field::Blob(bytes) => {
                let here = if bytes.len() <= FIELD_BYTES {
                    field[..bytes.len()].copy_from_slice(bytes);
                    field_at
                } else {
                    if !payload.len().is_multiple_of(BLOCK_ALIGN) {
                        payload.push(0);
                    }
                    let here = payload_start + payload.len() as u64;
                    field.copy_from_slice(&here.to_le_bytes());
                    payload.extend_from_slice(bytes);
                    here
                };
                match entry.tag {
                    TAG_TILE_OFFSETS => spots.offsets = here,
                    TAG_TILE_BYTE_COUNTS => spots.counts = here,
                    TAG_SUB_IFDS => spots.sub_ifds = Some(here),
                    _ => {}
                }
            }
        }
    }
    assert!(
        spots.offsets != 0 && spots.counts != 0,
        "a TIFF page lists its tiles"
    );
    let mut block = Vec::with_capacity(head + payload.len());
    block.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (entry, field) in entries.iter().zip(fields.as_chunks::<FIELD_BYTES>().0) {
        block.extend_from_slice(&entry.tag.to_le_bytes());
        block.extend_from_slice(&entry.kind.to_le_bytes());
        block.extend_from_slice(&entry.count.to_le_bytes());
        block.extend_from_slice(field);
    }
    block.extend_from_slice(&0u64.to_le_bytes());
    block.extend_from_slice(&payload);
    (block, spots)
}

pub(crate) fn description(spec: &ImageSpec<'_>) -> String {
    let level = spec.levels[0];
    let extent = level.extent();
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    xml.push_str(
        "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n <Image ID=\"Image:0\"",
    );
    xml.push_str(&format!(" Name=\"{}\">\n", escape(spec.name)));
    xml.push_str(&format!(
        "  <Pixels ID=\"Pixels:0\" DimensionOrder=\"XYZCT\" Type=\"{}\" SizeX=\"{}\" SizeY=\"{}\" SizeZ=\"{}\" SizeC=\"{}\" SizeT=\"{}\"",
        kind(spec.sample),
        extent.x(),
        extent.y(),
        extent.z(),
        spec.channels.len(),
        spec.frames
    ));
    if spec.calibration.measured() {
        let basis = level.basis();
        xml.push_str(&format!(
            " PhysicalSizeX=\"{}\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"{}\" PhysicalSizeYUnit=\"µm\" PhysicalSizeZ=\"{}\" PhysicalSizeZUnit=\"µm\"",
            number(basis.step(0)),
            number(basis.step(1)),
            number(basis.step(2))
        ));
    }
    if let Some(interval) = spec.interval {
        xml.push_str(&format!(
            " TimeIncrement=\"{}\" TimeIncrementUnit=\"s\"",
            number(interval)
        ));
    }
    xml.push_str(">\n");
    for (index, channel) in spec.channels.iter().enumerate() {
        xml.push_str(&format!(
            "   <Channel ID=\"Channel:0:{index}\" Name=\"{}\" Color=\"{}\" SamplesPerPixel=\"1\"/>\n",
            escape(channel.name),
            color(channel.tint)
        ));
    }
    xml.push_str(&plane_positions(spec));
    xml.push_str(&channels(spec));
    xml.push_str("  </Pixels>\n </Image>\n");
    xml.push_str(&annotations(spec));
    xml.push_str("</OME>");
    xml
}

fn plane_positions(spec: &ImageSpec<'_>) -> String {
    let level = spec.levels[0];
    let extent = level.extent();
    let basis = level.basis();
    let columns = [basis.column(0), basis.column(1), basis.column(2)];
    let corner = level.origin();
    let middle = [0.5 * extent.x() as f32, 0.5 * extent.y() as f32];
    let mut data = String::new();
    let mut planes = String::new();
    let mut ifd = 0usize;
    for frame in 0..spec.frames {
        for channel in 0..spec.channels.len() {
            for z in 0..extent.z() {
                if spec.calibration.measured() {
                    let local = [middle[0], middle[1], z as f32 + 0.5];
                    let centre = [0, 1, 2].map(|axis| {
                        corner[axis]
                            + columns[0][axis] * local[0]
                            + columns[1][axis] * local[1]
                            + columns[2][axis] * local[2]
                            + spec.placement[axis]
                    });
                    planes.push_str(&format!(
                        "   <Plane TheT=\"{frame}\" TheC=\"{channel}\" TheZ=\"{z}\" PositionX=\"{}\" PositionXUnit=\"µm\" PositionY=\"{}\" PositionYUnit=\"µm\" PositionZ=\"{}\" PositionZUnit=\"µm\"/>\n",
                        located(centre[0]),
                        located(centre[1]),
                        located(centre[2])
                    ));
                } else {
                    planes.push_str(&format!(
                        "   <Plane TheT=\"{frame}\" TheC=\"{channel}\" TheZ=\"{z}\"/>\n"
                    ));
                }
                data.push_str(&format!(
                    "   <TiffData IFD=\"{ifd}\" PlaneCount=\"1\" FirstT=\"{frame}\" FirstC=\"{channel}\" FirstZ=\"{z}\"/>\n"
                ));
                ifd += 1;
            }
        }
    }
    format!("{data}{planes}")
}

fn channels(spec: &ImageSpec<'_>) -> String {
    let mut values = String::new();
    for (channel, held) in spec.channels.iter().enumerate() {
        for (key, value) in [
            ("data.min", held.data.min()),
            ("data.max", held.data.max()),
            ("window.min", held.window.min()),
            ("window.max", held.window.max()),
        ] {
            values.push_str(&format!(
                "   <Value><M>channel.{channel}.{key}</M><S>{}</S></Value>\n",
                number(value)
            ));
        }
    }
    values
}

fn annotations(spec: &ImageSpec<'_>) -> String {
    let mut xml = String::from(
        " <StructuredAnnotations>\n  <MapAnnotation ID=\"Annotation:grana\" Namespace=\"urn:grana:archive\">\n",
    );
    let mut entries = grana_archive::annotations(spec.levels, spec.placement, spec.calibration);
    let tables = spec
        .channels
        .iter()
        .map(|channel| channel.color_table)
        .collect::<Vec<_>>();
    if let Some(text) = grana_archive::color_table_text(&tables) {
        entries.push((grana_archive::COLOR_TABLES.to_string(), text));
    }
    for (key, value) in entries {
        xml.push_str(&format!(
            "   <Value><M>{}</M><S>{}</S></Value>\n",
            escape(&key),
            escape(&value)
        ));
    }
    xml.push_str(&channels(spec));
    xml.push_str("   <Value><M>image</M><S>0</S></Value>\n");
    xml.push_str("  </MapAnnotation>\n </StructuredAnnotations>\n");
    xml
}

fn kind(sample: VoxelFormat) -> &'static str {
    match sample {
        VoxelFormat::U8 => "uint8",
        VoxelFormat::U16 => "uint16",
        VoxelFormat::U32 => "uint32",
        VoxelFormat::U64 => "uint64",
        VoxelFormat::I8 => "int8",
        VoxelFormat::I16 => "int16",
        VoxelFormat::I32 => "int32",
        VoxelFormat::I64 => "int64",
        VoxelFormat::F32 => "float",
        VoxelFormat::F64 => "double",
    }
}

fn color(tint: [f32; 3]) -> i32 {
    let [red, green, blue] = tint.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as i32);
    -16_777_216 + (red << 16) + (green << 8) + blue
}

fn number(value: f32) -> String {
    value.to_string()
}

fn located(value: f32) -> String {
    format!("{value:.6}")
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
