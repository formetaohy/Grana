use std::path::PathBuf;

use grana_czi::Image;
use grana_volume::{BrickCoord, Extent, PADDED_SIDE, VoxelData, VoxelFormat};

const WIDTH: u32 = 4;
const HEIGHT: u32 = 3;
const CHANNELS: u32 = 3;
const FRAMES: u32 = 1;
const DEPTH: u32 = 1;
const FILE_HEADER_BODY: usize = 512;
const METADATA_XML_OFFSET: usize = 256;
const ENTRY_MINIMUM: usize = 240;
const PYRAMID_WIDTH: u32 = 12;
const PYRAMID_HEIGHT: u32 = 6;
const COARSE_WIDTH: u32 = 4;
const COARSE_HEIGHT: u32 = 2;
const COARSE_TILE: u32 = 2;

struct Axis {
    code: [u8; 4],
    start: i32,
    size: u32,
    stored: u32,
}

impl Axis {
    fn new(code: [u8; 4], start: i32, size: u32) -> Self {
        Self {
            code,
            start,
            size,
            stored: 0,
        }
    }

    fn stored(code: [u8; 4], start: i32, size: u32, stored: u32) -> Self {
        Self {
            code,
            start,
            size,
            stored,
        }
    }
}

struct Plane {
    compression: i32,
    axes: Vec<Axis>,
    data: Vec<u8>,
}

#[test]
fn reads_a_written_file() {
    let path = write();
    let image = Image::open(&path, 0).expect("the written file opens");
    let pyramid = image.pyramid();
    let level = pyramid.level(0);
    assert_eq!(
        (level.extent().x(), level.extent().y(), level.extent().z()),
        (WIDTH, HEIGHT, DEPTH)
    );
    assert_eq!(pyramid.channel_count(), CHANNELS as usize);
    assert_eq!(pyramid.frame_count(), FRAMES);
    assert_eq!(pyramid.frame_interval(), 2.5);
    assert_eq!(
        (
            level.spacing().x(),
            level.spacing().y(),
            level.spacing().z()
        ),
        (1.0, 1.0, 2.0)
    );
    let first = &pyramid.channels()[0];
    assert_eq!(first.name(), "Red");
    assert_eq!(first.tint(), [1.0, 0.0, 0.0]);
    assert_eq!(first.format(), VoxelFormat::U16);
    assert!((first.window().max() - 0.5 * f32::from(u16::MAX)).abs() < 1.0);
    let second = &pyramid.channels()[1];
    assert!((second.window().min() - 0.25 * f32::from(u16::MAX)).abs() < 1.0);
    assert!((second.window().max() - f32::from(u16::MAX)).abs() < 1.0);
    assert!(pyramid.channels()[2].window().span() > 0.0);
    for channel in 0..CHANNELS as usize {
        let data = image
            .read_region(0, 0, channel, 0..WIDTH, 0..HEIGHT, 0..DEPTH)
            .expect("the plane reads");
        let VoxelData::U16(values) = data else {
            panic!("the plane is sixteen bit");
        };
        assert_eq!(values, expected(channel as u16));
    }
    let brick = image
        .read_brick(0, 0, BrickCoord::new(0, 0, 0))
        .expect("the brick reads");
    assert_eq!(brick.channels().len(), CHANNELS as usize);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn reads_a_pyramid() {
    let path = write_pyramid();
    let image = Image::open(&path, 0).expect("the pyramid opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.channel_count(), 1);
    let base = pyramid.level(0);
    assert_eq!(
        (base.extent().x(), base.extent().y(), base.extent().z()),
        (PYRAMID_WIDTH, PYRAMID_HEIGHT, 1)
    );
    assert_eq!(
        (base.spacing().x(), base.spacing().y(), base.spacing().z()),
        (1.0, 1.0, 2.0)
    );
    let coarse = pyramid.level(1);
    assert_eq!(
        (
            coarse.extent().x(),
            coarse.extent().y(),
            coarse.extent().z()
        ),
        (COARSE_WIDTH, COARSE_HEIGHT, 1)
    );
    assert_eq!(
        (
            coarse.spacing().x(),
            coarse.spacing().y(),
            coarse.spacing().z()
        ),
        (3.0, 3.0, 2.0)
    );
    let VoxelData::U8(base_samples) = image
        .read_region(0, 0, 0, 0..PYRAMID_WIDTH, 0..PYRAMID_HEIGHT, 0..1)
        .expect("the base plane reads")
    else {
        panic!("the base plane is eight bit");
    };
    assert_eq!(base_samples, pyramid_base_samples());
    let VoxelData::U8(coarse_samples) = image
        .read_region(0, 1, 0, 0..COARSE_WIDTH, 0..COARSE_HEIGHT, 0..1)
        .expect("the coarse plane reads")
    else {
        panic!("the coarse plane is eight bit");
    };
    assert_eq!(coarse_samples, pyramid_coarse_samples());
    let VoxelData::U8(second_tile) = image
        .read_region(0, 1, 0, 2..COARSE_WIDTH, 0..COARSE_HEIGHT, 0..1)
        .expect("the second coarse tile reads")
    else {
        panic!("the second coarse tile is eight bit");
    };
    assert_eq!(second_tile, vec![20, 21, 22, 23]);
    let brick = image
        .read_brick(0, 1, BrickCoord::new(0, 0, 0))
        .expect("the coarse brick reads");
    assert_eq!(brick.channels().len(), 1);
    let VoxelData::U8(brick_samples) = &brick.channels()[0] else {
        panic!("the coarse brick is eight bit");
    };
    assert_eq!(
        brick_samples.len(),
        (PADDED_SIDE * PADDED_SIDE * PADDED_SIDE) as usize
    );
    assert_eq!(brick_samples[0], 10);
    assert_eq!(brick_samples[3], 20);
    assert_eq!(brick_samples[4], 21);
    assert!(image.read_region(0, 2, 0, 0..1, 0..1, 0..1).is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn refuses_files_that_are_not_czi() {
    let path = std::env::temp_dir().join("grana-not-a-czi.czi");
    std::fs::write(&path, b"II*\0not a czi").expect("the file writes");
    assert!(Image::open(&path, 0).is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn reads_every_scene() {
    let path = write_scenes();
    let first = Image::open(&path, 0).expect("the first scene opens");
    assert_eq!(first.scene(), 0);
    assert_eq!(first.scenes().len(), 2);
    assert_eq!(first.scenes()[0].name(), Some("Overview"));
    assert_eq!(first.scenes()[1].name(), Some("Detail"));
    assert_eq!(
        grana_czi::scene_count(&path).expect("the scene count reads the directory"),
        2,
        "the scene count needs no metadata"
    );
    assert_eq!(first.scenes()[1].extent(), Extent::new(2, 2, 1));
    assert_eq!(first.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let VoxelData::U8(values) = first
        .read_region(0, 0, 0, 0..4, 0..3, 0..1)
        .expect("the first scene reads")
    else {
        panic!("the scene stores eight bit voxels");
    };
    assert_eq!(values, (0..12).collect::<Vec<u8>>());
    let second = Image::open(&path, 1).expect("the second scene opens");
    assert_eq!(second.scene(), 1);
    assert_eq!(second.pyramid().level(0).extent(), Extent::new(2, 2, 1));
    let VoxelData::U8(values) = second
        .read_region(0, 0, 0, 0..2, 0..2, 0..1)
        .expect("the second scene reads")
    else {
        panic!("the scene stores eight bit voxels");
    };
    assert_eq!(values, vec![100, 101, 102, 103]);
    assert!(Image::open(&path, 2).is_err());
    let _ = std::fs::remove_file(&path);
}

fn expected(channel: u16) -> Vec<u16> {
    (0..WIDTH * HEIGHT)
        .map(|index| channel * 1000 + index as u16)
        .collect()
}

fn samples(channel: u16) -> Vec<u8> {
    expected(channel)
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn write() -> PathBuf {
    let xml = xml();
    let metadata_length = METADATA_XML_OFFSET + xml.len();
    let entry_length = 32 + axes(0).len() * 20;
    let directory_body = 128 + CHANNELS as usize * entry_length;
    let metadata = 32 + FILE_HEADER_BODY;
    let directory = metadata + 32 + metadata_length;
    let first_subblock = directory + 32 + directory_body;

    let payloads = (0..CHANNELS).map(payload).collect::<Vec<_>>();
    let mut file = segment(b"ZISRAWFILE", &[0u8; FILE_HEADER_BODY]);
    file[32..36].copy_from_slice(&1i32.to_le_bytes());
    file.extend_from_slice(&segment(b"ZISRAWMETADATA", &metadata_body(&xml)));
    let mut positions = Vec::new();
    let mut position = first_subblock;
    for payload in &payloads {
        positions.push(position);
        position += payload.len();
    }
    let mut content = vec![0u8; 128];
    content[..4].copy_from_slice(&(CHANNELS as i32).to_le_bytes());
    for (channel, position) in positions.iter().enumerate() {
        content.extend_from_slice(&entry(
            1,
            compression(channel as u32),
            *position as u64,
            &axes(channel as u32),
        ));
    }
    file.extend_from_slice(&segment(b"ZISRAWDIRECTORY", &content));
    for payload in &payloads {
        file.extend_from_slice(payload);
    }
    patch(&mut file, 52, directory as u64);
    patch(&mut file, 60, metadata as u64);
    let path = std::env::temp_dir().join("grana-written.czi");
    std::fs::write(&path, &file).expect("the file writes");
    path
}

fn patch(file: &mut [u8], at: usize, value: u64) {
    file[32 + at..32 + at + 8].copy_from_slice(&(value as i64).to_le_bytes());
}

fn segment(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 32];
    bytes[..id.len()].copy_from_slice(id);
    bytes[16..24].copy_from_slice(&(body.len() as i64).to_le_bytes());
    bytes[24..32].copy_from_slice(&(body.len() as i64).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn metadata_body(xml: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.resize(METADATA_XML_OFFSET, 0);
    body.extend_from_slice(xml.as_bytes());
    body
}

fn axes(channel: u32) -> Vec<Axis> {
    vec![
        Axis::new(*b"X\0\0\0", 0, WIDTH),
        Axis::new(*b"Y\0\0\0", 0, HEIGHT),
        Axis::new(*b"Z\0\0\0", 0, DEPTH),
        Axis::new(*b"C\0\0\0", channel as i32, 1),
        Axis::new(*b"T\0\0\0", 0, 1),
    ]
}

fn entry(pixel: i32, compression: i32, position: u64, axes: &[Axis]) -> Vec<u8> {
    let mut bytes = vec![0u8; 32];
    bytes[..2].copy_from_slice(b"DV");
    bytes[2..6].copy_from_slice(&pixel.to_le_bytes());
    bytes[6..14].copy_from_slice(&(position as i64).to_le_bytes());
    bytes[18..22].copy_from_slice(&compression.to_le_bytes());
    bytes[28..32].copy_from_slice(&(axes.len() as i32).to_le_bytes());
    for axis in axes {
        let mut fields = vec![0u8; 20];
        fields[..4].copy_from_slice(&axis.code);
        fields[4..8].copy_from_slice(&axis.start.to_le_bytes());
        fields[8..12].copy_from_slice(&(axis.size as i32).to_le_bytes());
        fields[16..20].copy_from_slice(&(axis.stored as i32).to_le_bytes());
        bytes.extend_from_slice(&fields);
    }
    bytes
}

fn compression(channel: u32) -> i32 {
    match channel {
        0 => 0,
        1 => 5,
        _ => 6,
    }
}

fn payload(channel: u32) -> Vec<u8> {
    let compression = compression(channel);
    let samples = samples(channel as u16);
    let data = match compression {
        0 => samples,
        5 => zstd::bulk::compress(&samples, 3).expect("the plane compresses"),
        _ => {
            let mut packed =
                zstd::bulk::compress(&pack_hi_lo(&samples), 3).expect("the plane compresses");
            let mut header = vec![3u8, 1, 1];
            header.append(&mut packed);
            header
        }
    };
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&(data.len() as u64).to_le_bytes());
    body.extend_from_slice(&entry(1, compression, 0, &axes(channel)));
    body.resize(16 + ENTRY_MINIMUM, 0);
    body.extend_from_slice(&data);
    segment(b"ZISRAWSUBBLOCK", &body)
}

fn pack_hi_lo(samples: &[u8]) -> Vec<u8> {
    let (words, _) = samples.as_chunks::<2>();
    let mut packed = words.iter().map(|word| word[0]).collect::<Vec<_>>();
    packed.extend(words.iter().map(|word| word[1]));
    packed
}

fn xml() -> String {
    format!(
        "<ImageDocument><Metadata><Information><Image>\
<SizeX>{WIDTH}</SizeX><SizeY>{HEIGHT}</SizeY><SizeZ>{DEPTH}</SizeZ>\
<SizeC>{CHANNELS}</SizeC><SizeT>{FRAMES}</SizeT>\
<Dimensions><Channels>\
<Channel Id=\"Channel:1\"><Color>#FFFF0000</Color><Fluor>Red</Fluor></Channel>\
<Channel Id=\"Channel:2\"><Color>#FF00FF00</Color><Fluor>Green</Fluor></Channel>\
<Channel Id=\"Channel:3\"><Color>#FF0000FF</Color><Fluor>Blue</Fluor></Channel>\
</Channels>\
<T><Positions><Interval><Increment>2.5</Increment></Interval></Positions></T>\
</Dimensions></Image></Information>\
<Scaling><Items>\
<Distance Id=\"X\"><Value>1E-06</Value><DefaultUnitFormat>um</DefaultUnitFormat></Distance>\
<Distance Id=\"Y\"><Value>1E-06</Value></Distance>\
<Distance Id=\"Z\"><Value>2E-06</Value></Distance>\
</Items></Scaling>\
<DisplaySetting><Channels>\
<Channel Id=\"Channel:1\" Name=\"Red\"><Low>0</Low><High>0.5</High></Channel>\
<Channel Id=\"Channel:2\" Name=\"Green\"><Low>0.25</Low><High>1</High></Channel>\
</Channels></DisplaySetting>\
</Metadata></ImageDocument>"
    )
}

fn write_pyramid() -> PathBuf {
    let xml = pyramid_xml();
    let metadata_length = METADATA_XML_OFFSET + xml.len();
    let planes = pyramid_planes();
    let directory_body = 128
        + planes
            .iter()
            .map(|plane| 32 + plane.axes.len() * 20)
            .sum::<usize>();
    let metadata = 32 + FILE_HEADER_BODY;
    let directory = metadata + 32 + metadata_length;
    let first_subblock = directory + 32 + directory_body;
    let payloads = planes.iter().map(subblock).collect::<Vec<_>>();
    let mut positions = Vec::new();
    let mut position = first_subblock;
    for payload in &payloads {
        positions.push(position);
        position += payload.len();
    }
    let mut content = vec![0u8; 128];
    content[..4].copy_from_slice(&(planes.len() as i32).to_le_bytes());
    for (plane, position) in planes.iter().zip(positions.iter()) {
        content.extend_from_slice(&entry(0, plane.compression, *position as u64, &plane.axes));
    }
    let mut file = segment(b"ZISRAWFILE", &[0u8; FILE_HEADER_BODY]);
    file[32..36].copy_from_slice(&1i32.to_le_bytes());
    file.extend_from_slice(&segment(b"ZISRAWMETADATA", &metadata_body(&xml)));
    file.extend_from_slice(&segment(b"ZISRAWDIRECTORY", &content));
    for payload in &payloads {
        file.extend_from_slice(payload);
    }
    patch(&mut file, 52, directory as u64);
    patch(&mut file, 60, metadata as u64);
    let path = std::env::temp_dir().join("grana-pyramid.czi");
    std::fs::write(&path, &file).expect("the file writes");
    path
}

fn pyramid_planes() -> Vec<Plane> {
    vec![
        Plane {
            compression: 0,
            axes: pyramid_axes(
                0,
                0,
                PYRAMID_WIDTH,
                PYRAMID_HEIGHT,
                PYRAMID_WIDTH,
                PYRAMID_HEIGHT,
            ),
            data: pyramid_base_samples(),
        },
        Plane {
            compression: 0,
            axes: pyramid_axes(
                0,
                0,
                PYRAMID_WIDTH / 2,
                PYRAMID_HEIGHT,
                COARSE_TILE,
                COARSE_TILE,
            ),
            data: vec![10, 11, 12, 13],
        },
        Plane {
            compression: 0,
            axes: pyramid_axes(
                1,
                (PYRAMID_WIDTH / 2) as i32,
                PYRAMID_WIDTH / 2,
                PYRAMID_HEIGHT,
                COARSE_TILE,
                COARSE_TILE,
            ),
            data: vec![20, 21, 22, 23],
        },
    ]
}

fn pyramid_axes(
    mosaic: i32,
    x: i32,
    width: u32,
    height: u32,
    stored_width: u32,
    stored_height: u32,
) -> Vec<Axis> {
    vec![
        Axis::stored(*b"X\0\0\0", x, width, stored_width),
        Axis::stored(*b"Y\0\0\0", 0, height, stored_height),
        Axis::new(*b"Z\0\0\0", 0, 1),
        Axis::new(*b"C\0\0\0", 0, 1),
        Axis::new(*b"T\0\0\0", 0, 1),
        Axis::new(*b"M\0\0\0", mosaic, 1),
    ]
}

fn subblock(plane: &Plane) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&(plane.data.len() as u64).to_le_bytes());
    body.extend_from_slice(&entry(0, plane.compression, 0, &plane.axes));
    body.resize(16 + ENTRY_MINIMUM, 0);
    body.extend_from_slice(&plane.data);
    segment(b"ZISRAWSUBBLOCK", &body)
}

fn pyramid_base_samples() -> Vec<u8> {
    (0..PYRAMID_WIDTH * PYRAMID_HEIGHT)
        .map(|index| index as u8)
        .collect()
}

fn pyramid_coarse_samples() -> Vec<u8> {
    vec![10, 11, 20, 21, 12, 13, 22, 23]
}

fn pyramid_xml() -> String {
    format!(
        "<ImageDocument><Metadata><Information><Image>\
<SizeX>{PYRAMID_WIDTH}</SizeX><SizeY>{PYRAMID_HEIGHT}</SizeY><SizeZ>1</SizeZ>\
<SizeC>1</SizeC><SizeT>1</SizeT>\
<Dimensions><Channels>\
<Channel Id=\"Channel:1\"><Color>#FFFF0000</Color><Fluor>Red</Fluor></Channel>\
</Channels></Dimensions></Image></Information>\
<Scaling><Items>\
<Distance Id=\"X\"><Value>1E-06</Value></Distance>\
<Distance Id=\"Y\"><Value>1E-06</Value></Distance>\
<Distance Id=\"Z\"><Value>2E-06</Value></Distance>\
</Items></Scaling>\
</Metadata></ImageDocument>"
    )
}

fn write_scenes() -> PathBuf {
    let xml = scenes_xml();
    let metadata_length = METADATA_XML_OFFSET + xml.len();
    let planes = scene_planes();
    let directory_body = 128
        + planes
            .iter()
            .map(|plane| 32 + plane.axes.len() * 20)
            .sum::<usize>();
    let metadata = 32 + FILE_HEADER_BODY;
    let directory = metadata + 32 + metadata_length;
    let first_subblock = directory + 32 + directory_body;
    let payloads = planes.iter().map(subblock).collect::<Vec<_>>();
    let mut positions = Vec::new();
    let mut position = first_subblock;
    for payload in &payloads {
        positions.push(position);
        position += payload.len();
    }
    let mut content = vec![0u8; 128];
    content[..4].copy_from_slice(&(planes.len() as i32).to_le_bytes());
    for (plane, position) in planes.iter().zip(positions.iter()) {
        content.extend_from_slice(&entry(0, plane.compression, *position as u64, &plane.axes));
    }
    let mut file = segment(b"ZISRAWFILE", &[0u8; FILE_HEADER_BODY]);
    file[32..36].copy_from_slice(&1i32.to_le_bytes());
    file.extend_from_slice(&segment(b"ZISRAWMETADATA", &metadata_body(&xml)));
    file.extend_from_slice(&segment(b"ZISRAWDIRECTORY", &content));
    for payload in &payloads {
        file.extend_from_slice(payload);
    }
    patch(&mut file, 52, directory as u64);
    patch(&mut file, 60, metadata as u64);
    let path = std::env::temp_dir().join("grana-scenes.czi");
    std::fs::write(&path, &file).expect("the file writes");
    path
}

fn scene_planes() -> Vec<Plane> {
    vec![
        Plane {
            compression: 0,
            axes: scene_axes(0, 4, 3),
            data: (0..12).collect(),
        },
        Plane {
            compression: 0,
            axes: scene_axes(1, 2, 2),
            data: vec![100, 101, 102, 103],
        },
    ]
}

fn scene_axes(scene: i32, width: u32, height: u32) -> Vec<Axis> {
    vec![
        Axis::new(*b"X\0\0\0", 0, width),
        Axis::new(*b"Y\0\0\0", 0, height),
        Axis::new(*b"Z\0\0\0", 0, 1),
        Axis::new(*b"C\0\0\0", 0, 1),
        Axis::new(*b"T\0\0\0", 0, 1),
        Axis::new(*b"S\0\0\0", scene, 1),
    ]
}

fn scenes_xml() -> String {
    "<ImageDocument><Metadata><Information>\
<Image Name=\"Overview\"><SizeX>4</SizeX><SizeY>3</SizeY><SizeZ>1</SizeZ><SizeC>1</SizeC><SizeT>1</SizeT>\
<Dimensions><Channels>\
<Channel Id=\"Channel:1\"><Color>#FFFF0000</Color><Fluor>Red</Fluor></Channel>\
</Channels></Dimensions></Image>\
<Image Name=\"Detail\"><SizeX>2</SizeX><SizeY>2</SizeY><SizeZ>1</SizeZ><SizeC>1</SizeC><SizeT>1</SizeT>\
</Image></Information>\
<Scaling><Items>\
<Distance Id=\"X\"><Value>1E-06</Value></Distance>\
<Distance Id=\"Y\"><Value>1E-06</Value></Distance>\
<Distance Id=\"Z\"><Value>2E-06</Value></Distance>\
</Items></Scaling>\
</Metadata></ImageDocument>"
        .to_string()
}
