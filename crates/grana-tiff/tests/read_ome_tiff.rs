use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::write::ZlibEncoder;
use grana_tiff::Image;
use grana_volume::{BrickCoord, Extent, Intensity, Spacing, VoxelData, VoxelFormat};

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
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SUB_IFDS: u16 = 330;
const TAG_SAMPLE_FORMAT: u16 = 339;

const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;
const KIND_IFD: u16 = 13;
const KIND_LONG8: u16 = 16;
const KIND_IFD8: u16 = 18;

const COMPRESSION_NONE: u16 = 1;
const COMPRESSION_DEFLATE: u16 = 8;

#[test]
fn reads_pyramid_with_tiles_and_sub_images() {
    let temp = TempDir::new("grana-tiff-pyramid");
    let path = temp.path().join("pyramid.ome.tif");
    fs::write(&path, pyramid_image()).expect("the fixture is written");
    let image = Image::open(&path).expect("the pyramid opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 1);
    assert_eq!(pyramid.frame_interval(), 3.0);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(pyramid.channels()[1].name(), "RFP");
    assert_eq!(pyramid.channels()[1].tint(), [1.0, 0.0, 0.0]);
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(0.0, 65535.0)
    );
    assert_eq!(pyramid.channels()[0].format(), VoxelFormat::U16);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.level(0).extent(), Extent::new(8, 6, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.25, 2.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(4, 3, 2));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 0.5, 2.0));

    let data = image
        .read_region(0, 0, 1, 2..6, 1..4, 0..2)
        .expect("a region reads");
    let VoxelData::U16(values) = data else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(values.len(), 4 * 3 * 2);
    for z in 0..2u32 {
        for y in 1..4u32 {
            for x in 2..6u32 {
                let expected = 1000 * z + 100 + 10 * y + x;
                let index = ((z * 3 + (y - 1)) * 4 + (x - 2)) as usize;
                assert_eq!(u32::from(values[index]), expected, "voxel {x},{y},{z}");
            }
        }
    }

    let brick = image
        .read_brick(0, 0, BrickCoord::new(0, 0, 0))
        .expect("a brick reads");
    let VoxelData::U16(first) = &brick.channels()[0] else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(first.len(), 34 * 34 * 34);
    assert_eq!(first[0], 0);
    let last = 33 + 34 * (33 + 34 * 33);
    assert_eq!(first[last], 1057);

    let coarse = image
        .read_region(0, 1, 0, 0..4, 0..3, 0..2)
        .expect("a coarse region reads");
    let VoxelData::U16(values) = coarse else {
        panic!("the fixture stores uint16 voxels");
    };
    for z in 0..2u32 {
        for y in 0..3u32 {
            for x in 0..4u32 {
                let index = ((z * 3 + y) * 4 + x) as usize;
                assert_eq!(u32::from(values[index]), 5000 + 10 * y + x);
            }
        }
    }
}

#[test]
fn reads_planar_channels_from_big_tiff() {
    let temp = TempDir::new("grana-tiff-planar");
    let path = temp.path().join("planar.ome.tiff");
    fs::write(&path, planar_image()).expect("the fixture is written");
    let image = Image::open(&path).expect("the planar image opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.level(0).extent(), Extent::new(4, 3, 1));
    for channel in 0..2u32 {
        let data = image
            .read_region(0, 0, channel as usize, 0..4, 0..3, 0..1)
            .expect("a channel reads");
        let VoxelData::U16(values) = data else {
            panic!("the fixture stores uint16 voxels");
        };
        for y in 0..3u32 {
            for x in 0..4u32 {
                let expected = 100 * channel + 10 * y + x;
                assert_eq!(
                    u32::from(values[(y * 4 + x) as usize]),
                    expected,
                    "channel {channel} voxel {x},{y}"
                );
            }
        }
    }
}

#[test]
fn reads_a_pillow_ome_tiff() {
    let temp = TempDir::new("grana-tiff-pillow");
    let path = temp.path().join("pillow.ome.tif");
    fs::write(&path, include_bytes!("ome.tif")).expect("the fixture is written");
    let image = Image::open(&path).expect("the fixture opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 1);
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level(0).extent(), Extent::new(32, 32, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 1.0));
    assert_eq!(pyramid.channels()[0].name(), "GFP");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 1.0, 0.0]);
    let data = image
        .read_region(0, 0, 0, 0..32, 0..32, 0..2)
        .expect("the volume reads");
    let VoxelData::U8(values) = data else {
        panic!("the fixture stores bytes");
    };
    for z in 0..2u32 {
        for y in 0..32u32 {
            for x in 0..32u32 {
                let index = ((z * 32 + y) * 32 + x) as usize;
                assert_eq!(
                    u32::from(values[index]),
                    (z * 97 + y * 5 + x * 3) % 256,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }
}

#[test]
fn reads_a_series_split_over_several_files() {
    let temp = TempDir::new("grana-tiff-series");
    let first = temp.path().join("field.c1.ome.tif");
    let second = temp.path().join("field.c2.ome.tif");
    fs::write(&first, channel_image(&first, 0)).expect("the first file is written");
    fs::write(&second, channel_image(&second, 1)).expect("the second file is written");
    let image = Image::open(&first).expect("the series opens from its first file");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.level(0).extent(), Extent::new(4, 3, 1));
    for channel in 0..2u32 {
        let data = image
            .read_region(0, 0, channel as usize, 0..4, 0..3, 0..1)
            .expect("a channel reads");
        let VoxelData::U8(values) = data else {
            panic!("the fixture stores bytes");
        };
        for y in 0..3u32 {
            for x in 0..4u32 {
                let expected = 50 * channel + 10 * y + x;
                assert_eq!(
                    u32::from(values[(y * 4 + x) as usize]),
                    expected,
                    "channel {channel} voxel {x},{y}"
                );
            }
        }
    }
}

fn channel_image(path: &Path, channel: u32) -> Vec<u8> {
    let mut tiff = Tiff::new(false);
    let mut raw = Vec::new();
    for y in 0..3u32 {
        for x in 0..4u32 {
            raw.push((50 * channel + 10 * y + x) as u8);
        }
    }
    let count = raw.len() as u64;
    let offset = tiff.append(&raw);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let other = if channel == 0 {
        "field.c2.ome.tif"
    } else {
        "field.c1.ome.tif"
    };
    let xml = format!(
        "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\
<Image Name=\"Field\"><Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"4\" SizeY=\"3\" SizeZ=\"1\" SizeC=\"2\" SizeT=\"1\">\
<TiffData IFD=\"0\" PlaneCount=\"1\" FirstC=\"{channel}\"><UUID FileName=\"{name}\"/></TiffData>\
<TiffData IFD=\"0\" PlaneCount=\"1\" FirstC=\"{}\"><UUID FileName=\"{other}\"/></TiffData>\
</Pixels></Image></OME>",
        1 - channel
    );
    let page = tiff.directory(vec![
        longs(TAG_IMAGE_WIDTH, &[4]),
        longs(TAG_IMAGE_LENGTH, &[3]),
        shorts(TAG_BITS_PER_SAMPLE, &[8]),
        shorts(TAG_COMPRESSION, &[COMPRESSION_NONE]),
        shorts(TAG_PHOTOMETRIC, &[1]),
        offsets(false, TAG_STRIP_OFFSETS, &[offset]),
        shorts(TAG_SAMPLES_PER_PIXEL, &[1]),
        shorts(TAG_ROWS_PER_STRIP, &[3]),
        offsets(false, TAG_STRIP_BYTE_COUNTS, &[count]),
        shorts(TAG_PLANAR_CONFIGURATION, &[1]),
        shorts(TAG_SAMPLE_FORMAT, &[1]),
        ascii(TAG_IMAGE_DESCRIPTION, &xml),
    ]);
    tiff.chain(&[page]);
    tiff.finish()
}

fn pyramid_image() -> Vec<u8> {
    let mut tiff = Tiff::new(false);
    let width = 8u32;
    let height = 6u32;
    let tile_width = 4u32;
    let tile_height = 3u32;
    let samples = 2u16;
    let mut pages = Vec::new();
    for z in 0..2u32 {
        let mut tiles = Vec::new();
        for tile_y in 0..2u32 {
            for tile_x in 0..2u32 {
                let mut raw = Vec::new();
                for y in 0..tile_height {
                    for x in 0..tile_width {
                        let px = tile_x * tile_width + x;
                        let py = tile_y * tile_height + y;
                        for channel in 0..2u32 {
                            let value = (1000 * z + 100 * channel + 10 * py + px) as u16;
                            raw.extend_from_slice(&value.to_le_bytes());
                        }
                    }
                }
                let encoded = deflate(&raw);
                let count = encoded.len() as u64;
                let offset = tiff.append(&encoded);
                tiles.push((offset, count));
            }
        }
        pages.push(tiles);
    }
    let mut sub_images = Vec::new();
    for _ in 0..2u32 {
        let mut raw = Vec::new();
        for y in 0..3u32 {
            for x in 0..4u32 {
                for channel in 0..2u32 {
                    let value = (5000 + 100 * channel + 10 * y + x) as u16;
                    raw.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        let count = raw.len() as u64;
        let offset = tiff.append(&raw);
        sub_images.push(tiff.directory(vec![
            longs(TAG_IMAGE_WIDTH, &[4]),
            longs(TAG_IMAGE_LENGTH, &[3]),
            shorts(TAG_BITS_PER_SAMPLE, &[16, 16]),
            shorts(TAG_COMPRESSION, &[COMPRESSION_NONE]),
            shorts(TAG_PHOTOMETRIC, &[1]),
            offsets(false, TAG_STRIP_OFFSETS, &[offset]),
            shorts(TAG_SAMPLES_PER_PIXEL, &[samples]),
            shorts(TAG_ROWS_PER_STRIP, &[3]),
            offsets(false, TAG_STRIP_BYTE_COUNTS, &[count]),
            shorts(TAG_PLANAR_CONFIGURATION, &[1]),
            shorts(TAG_SAMPLE_FORMAT, &[1, 1]),
        ]));
    }
    let mut level_zero = Vec::new();
    for (index, tiles) in pages.iter().enumerate() {
        let tile_offsets = tiles.iter().map(|(offset, _)| *offset).collect::<Vec<_>>();
        let tile_counts = tiles.iter().map(|(_, count)| *count).collect::<Vec<_>>();
        let sub_image = tiff.offset(sub_images[index]);
        let mut entries = vec![
            longs(TAG_IMAGE_WIDTH, &[width]),
            longs(TAG_IMAGE_LENGTH, &[height]),
            shorts(TAG_BITS_PER_SAMPLE, &[16, 16]),
            shorts(TAG_COMPRESSION, &[COMPRESSION_DEFLATE]),
            shorts(TAG_PHOTOMETRIC, &[1]),
            longs(TAG_TILE_WIDTH, &[tile_width]),
            longs(TAG_TILE_LENGTH, &[tile_height]),
            offsets(false, TAG_TILE_OFFSETS, &tile_offsets),
            shorts(TAG_SAMPLES_PER_PIXEL, &[samples]),
            offsets(false, TAG_TILE_BYTE_COUNTS, &tile_counts),
            shorts(TAG_PLANAR_CONFIGURATION, &[1]),
            shorts(TAG_SAMPLE_FORMAT, &[1, 1]),
            ifds(false, TAG_SUB_IFDS, &[sub_image]),
        ];
        if index == 0 {
            entries.push(ascii(TAG_IMAGE_DESCRIPTION, PYRAMID_OME));
        }
        level_zero.push(tiff.directory(entries));
    }
    tiff.chain(&level_zero);
    tiff.finish()
}

fn planar_image() -> Vec<u8> {
    let mut tiff = Tiff::new(true);
    let mut pages = Vec::new();
    for channel in 0..2u32 {
        let mut raw = Vec::new();
        for y in 0..3u32 {
            for x in 0..4u32 {
                let value = (100 * channel + 10 * y + x) as u16;
                raw.extend_from_slice(&value.to_le_bytes());
            }
        }
        let count = raw.len() as u64;
        let offset = tiff.append(&raw);
        let mut entries = vec![
            longs(TAG_IMAGE_WIDTH, &[4]),
            longs(TAG_IMAGE_LENGTH, &[3]),
            shorts(TAG_BITS_PER_SAMPLE, &[16]),
            shorts(TAG_COMPRESSION, &[COMPRESSION_NONE]),
            shorts(TAG_PHOTOMETRIC, &[1]),
            offsets(true, TAG_STRIP_OFFSETS, &[offset]),
            shorts(TAG_SAMPLES_PER_PIXEL, &[1]),
            shorts(TAG_ROWS_PER_STRIP, &[3]),
            offsets(true, TAG_STRIP_BYTE_COUNTS, &[count]),
            shorts(TAG_PLANAR_CONFIGURATION, &[2]),
            shorts(TAG_SAMPLE_FORMAT, &[1]),
        ];
        if channel == 0 {
            entries.push(ascii(TAG_IMAGE_DESCRIPTION, PLANAR_OME));
        }
        pages.push(tiff.directory(entries));
    }
    tiff.chain(&pages);
    tiff.finish()
}

const PYRAMID_OME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OME xmlns="http://www.openmicroscopy.org/Schemas/OME/2016-06">
  <Image Name="Pyramid">
    <Pixels DimensionOrder="XYZCT" Type="uint16" SizeX="8" SizeY="6" SizeZ="2" SizeC="2" SizeT="1"
            PhysicalSizeX="0.5" PhysicalSizeY="0.25" PhysicalSizeZ="2" PhysicalSizeXUnit="µm"
            PhysicalSizeYUnit="µm" PhysicalSizeZUnit="µm" TimeIncrement="3" TimeIncrementUnit="s">
      <Channel ID="Channel:0:0" Name="DAPI" Color="-16776961" SamplesPerPixel="2"/>
      <Channel ID="Channel:0:1" Name="RFP" Color="-65536" SamplesPerPixel="2"/>
      <TiffData IFD="0" PlaneCount="2"/>
    </Pixels>
  </Image>
</OME>"#;

const PLANAR_OME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OME xmlns="http://www.openmicroscopy.org/Schemas/OME/2016-06">
  <Image Name="Planar">
    <Pixels DimensionOrder="XYZCT" Type="uint16" SizeX="4" SizeY="3" SizeZ="1" SizeC="2" SizeT="1">
      <Channel ID="Channel:0:0" Name="Green" Color="-16711936" SamplesPerPixel="1"/>
      <Channel ID="Channel:0:1" Name="Blue" Color="-16776961" SamplesPerPixel="1"/>
    </Pixels>
  </Image>
</OME>"#;

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("the tile compresses");
    encoder.finish().expect("the tile compresses")
}

struct Entry {
    tag: u16,
    kind: u16,
    data: Vec<u8>,
}

fn shorts(tag: u16, values: &[u16]) -> Entry {
    Entry {
        tag,
        kind: KIND_SHORT,
        data: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn longs(tag: u16, values: &[u32]) -> Entry {
    Entry {
        tag,
        kind: KIND_LONG,
        data: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn long8s(tag: u16, values: &[u64]) -> Entry {
    Entry {
        tag,
        kind: KIND_LONG8,
        data: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn ifds(big: bool, tag: u16, values: &[u64]) -> Entry {
    Entry {
        tag,
        kind: if big { KIND_IFD8 } else { KIND_IFD },
        data: values
            .iter()
            .flat_map(|value| {
                if big {
                    value.to_le_bytes().to_vec()
                } else {
                    (*value as u32).to_le_bytes().to_vec()
                }
            })
            .collect(),
    }
}

fn offsets(big: bool, tag: u16, values: &[u64]) -> Entry {
    if big {
        return long8s(tag, values);
    }
    let narrow = values.iter().map(|value| *value as u32).collect::<Vec<_>>();
    longs(tag, &narrow)
}

fn ascii(tag: u16, text: &str) -> Entry {
    let mut data = text.as_bytes().to_vec();
    data.push(0);
    Entry {
        tag,
        kind: KIND_ASCII,
        data,
    }
}

struct Tiff {
    big: bool,
    buffer: Vec<u8>,
    directories: Vec<usize>,
    next_positions: Vec<usize>,
}

impl Tiff {
    fn new(big: bool) -> Self {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"II");
        if big {
            buffer.extend_from_slice(&43u16.to_le_bytes());
            buffer.extend_from_slice(&8u16.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&0u64.to_le_bytes());
        } else {
            buffer.extend_from_slice(&42u16.to_le_bytes());
            buffer.extend_from_slice(&0u32.to_le_bytes());
        }
        Self {
            big,
            buffer,
            directories: Vec::new(),
            next_positions: Vec::new(),
        }
    }

    fn align(&mut self) {
        while !self.buffer.len().is_multiple_of(2) {
            self.buffer.push(0);
        }
    }

    fn append(&mut self, data: &[u8]) -> u64 {
        self.align();
        let offset = self.buffer.len() as u64;
        self.buffer.extend_from_slice(data);
        offset
    }

    fn directory(&mut self, mut entries: Vec<Entry>) -> usize {
        entries.sort_by_key(|entry| entry.tag);
        self.align();
        let offset = self.buffer.len();
        let count_size = if self.big { 8 } else { 2 };
        let entry_size = if self.big { 20 } else { 12 };
        let next_size = if self.big { 8 } else { 4 };
        let count = entries.len();
        self.buffer
            .resize(offset + count_size + count * entry_size + next_size, 0);
        let mut payloads = Vec::with_capacity(count);
        for entry in &entries {
            let inline = if self.big { 8 } else { 4 };
            if entry.data.len() > inline {
                payloads.push(Some(self.append(&entry.data)));
            } else {
                payloads.push(None);
            }
        }
        if self.big {
            put_u64(&mut self.buffer[offset..], count as u64);
        } else {
            put_u16(&mut self.buffer[offset..], count as u16);
        }
        for (index, (entry, payload)) in entries.iter().zip(payloads).enumerate() {
            let start = offset + count_size + index * entry_size;
            let data = &mut self.buffer[start..start + entry_size];
            put_u16(data, entry.tag);
            put_u16(&mut data[2..], entry.kind);
            let count = (entry.data.len() / kind_size(entry.kind)) as u64;
            if self.big {
                put_u64(&mut data[4..], count);
            } else {
                put_u32(&mut data[4..], count as u32);
            }
            let value_start = if self.big { 12 } else { 8 };
            match payload {
                Some(payload) => {
                    if self.big {
                        put_u64(&mut data[value_start..], payload);
                    } else {
                        put_u32(&mut data[value_start..], payload as u32);
                    }
                }
                None => {
                    data[value_start..value_start + entry.data.len()].copy_from_slice(&entry.data);
                }
            }
        }
        self.next_positions
            .push(offset + count_size + count * entry_size);
        self.directories.push(offset);
        self.directories.len() - 1
    }

    fn offset(&self, index: usize) -> u64 {
        self.directories[index] as u64
    }

    fn chain(&mut self, pages: &[usize]) {
        for pair in pages.windows(2) {
            let next = self.directories[pair[1]] as u64;
            let position = self.next_positions[pair[0]];
            if self.big {
                put_u64(&mut self.buffer[position..], next);
            } else {
                put_u32(&mut self.buffer[position..], next as u32);
            }
        }
        let first = self.directories[pages[0]] as u64;
        if self.big {
            put_u64(&mut self.buffer[8..], first);
        } else {
            put_u32(&mut self.buffer[4..], first as u32);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

fn kind_size(kind: u16) -> usize {
    match kind {
        KIND_ASCII => 1,
        KIND_SHORT => 2,
        KIND_LONG | KIND_IFD => 4,
        KIND_LONG8 | KIND_IFD8 => 8,
        _ => panic!("the fixture uses a known TIFF type"),
    }
}

fn put_u16(buffer: &mut [u8], value: u16) {
    buffer[..2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(buffer: &mut [u8], value: u32) {
    buffer[..4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(buffer: &mut [u8], value: u64) {
    buffer[..8].copy_from_slice(&value.to_le_bytes());
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).expect("the temporary directory is created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
