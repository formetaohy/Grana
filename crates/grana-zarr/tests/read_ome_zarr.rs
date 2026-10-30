use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{BrickCoord, Extent, Intensity, Spacing, VoxelData, VoxelFormat};
use grana_zarr::Image;

const SIDE: u32 = 34;

#[test]
fn reads_ome_zarr_v04_and_v05() {
    let temp = TempDir::new("grana-zarr");
    let v4 = temp.path().join("v04.zarr");
    write_v04(&v4);
    let v5 = temp.path().join("v05.zarr");
    write_v05(&v5);

    let image = Image::open(&v4).expect("the v0.4 dataset opens");
    assert_eq!(image.name(), "fixture");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 2);
    assert_eq!(pyramid.frame_interval(), 2.5);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(0.0, 65535.0)
    );
    assert_eq!(pyramid.channels()[0].window(), Intensity::new(0.0, 65535.0));
    assert_eq!(pyramid.channels()[0].format(), VoxelFormat::U16);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.level(0).extent(), Extent::new(5, 4, 3));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 4.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(3, 2, 2));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 4.0));

    let brick = image
        .read_brick(0, 0, BrickCoord::new(0, 0, 0))
        .expect("the first v0.4 frame reads");
    let VoxelData::U16(first) = &brick.channels()[0] else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(first.len(), (SIDE * SIDE * SIDE) as usize);
    for z in 0..3u32 {
        for y in 0..4u32 {
            for x in 0..5u32 {
                assert_eq!(voxel(first, x, y, z), (100 * z + 10 * y + x) as u16);
            }
        }
    }
    assert_eq!(voxel(first, SIDE - 2, SIDE - 2, SIDE - 2), 234);
    let VoxelData::U16(second) = &brick.channels()[1] else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(voxel(second, 0, 0, 0), 1000);

    let brick = image
        .read_brick(1, 0, BrickCoord::new(0, 0, 0))
        .expect("the second v0.4 frame reads");
    let VoxelData::U16(first) = &brick.channels()[0] else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(voxel(first, 0, 0, 0), 10_000);
    assert_eq!(voxel(first, 4, 3, 2), 10_234);
    assert_eq!(voxel(first, SIDE - 2, SIDE - 2, SIDE - 2), 10_234);
    let VoxelData::U16(second) = &brick.channels()[1] else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(voxel(second, 0, 0, 0), 11_000);

    let region = image
        .read_region(1, 0, 1, 1..4, 1..3, 0..2)
        .expect("a v0.4 region reads");
    let VoxelData::U16(values) = &region else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(values.len(), 3 * 2 * 2);
    assert_eq!(values[0], 11_011);
    assert_eq!(values[values.len() - 1], 11_123);
    assert!(image.read_region(2, 0, 0, 0..1, 0..1, 0..1).is_err());
    assert!(image.read_region(0, 0, 0, 0..6, 0..1, 0..1).is_err());

    let image = Image::open(&v5).expect("the v0.5 dataset opens");
    assert_eq!(image.name(), "fixture-v5");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 1);
    assert_eq!(pyramid.frame_count(), 1);
    assert_eq!(pyramid.channels()[0].name(), "RFP");
    assert_eq!(pyramid.channels()[0].tint(), [1.0, 0.0, 0.0]);
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(0.0, 255.0)
    );
    assert_eq!(pyramid.level(0).extent(), Extent::new(4, 3, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 4.0));
    let brick = image
        .read_brick(0, 0, BrickCoord::new(0, 0, 0))
        .expect("the v0.5 brick reads");
    let VoxelData::U8(data) = &brick.channels()[0] else {
        panic!("the fixture stores uint8 voxels");
    };
    for z in 0..2u32 {
        for y in 0..3u32 {
            for x in 0..4u32 {
                assert_eq!(voxel(data, x, y, z), (20 * z + 5 * y + x) as u8);
            }
        }
    }
    assert_eq!(voxel(data, 3, 2, 1), 33);
    assert_eq!(voxel(data, SIDE - 2, SIDE - 2, SIDE - 2), 33);
}

fn voxel<T: Copy>(data: &[T], x: u32, y: u32, z: u32) -> T {
    let x = (x + 1).min(SIDE - 1);
    let y = (y + 1).min(SIDE - 1);
    let z = (z + 1).min(SIDE - 1);
    data[(x + SIDE * (y + SIDE * z)) as usize]
}

fn write_v04(root: &Path) {
    write(root.join(".zgroup"), br#"{"zarr_format":2}"#);
    write(
        root.join(".zattrs"),
        br#"{
            "multiscales": [{
                "version": "0.4",
                "name": "fixture",
                "axes": [
                    {"name": "t", "type": "time", "unit": "second"},
                    {"name": "c", "type": "channel"},
                    {"name": "z", "type": "space", "unit": "micrometer"},
                    {"name": "y", "type": "space", "unit": "micrometer"},
                    {"name": "x", "type": "space", "unit": "micrometer"}
                ],
                "datasets": [
                    {"path": "0", "coordinateTransformations": [{"type": "scale", "scale": [2.5, 1.0, 4.0, 0.5, 0.5]}]},
                    {"path": "1", "coordinateTransformations": [{"type": "scale", "scale": [2.5, 1.0, 4.0, 1.0, 1.0]}]}
                ]
            }],
            "omero": {
                "channels": [
                    {"color": "0000FF", "window": {"min": 0, "max": 65535, "start": 0, "end": 65535}, "label": "DAPI"},
                    {"color": "00FF00", "window": {"min": 0, "max": 65535, "start": 0, "end": 65535}, "label": "GFP"}
                ]
            }
        }"#,
    );
    write(
        root.join("0").join(".zarray"),
        br#"{"zarr_format":2,"shape":[2,2,3,4,5],"chunks":[1,1,3,4,5],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}"#,
    );
    for time in 0..2u16 {
        for channel in 0..2u16 {
            let mut data = Vec::new();
            for z in 0..3u16 {
                for y in 0..4u16 {
                    for x in 0..5u16 {
                        let value = time * 10_000 + channel * 1000 + 100 * z + 10 * y + x;
                        data.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
            write(
                root.join("0")
                    .join(time.to_string())
                    .join(channel.to_string())
                    .join("0")
                    .join("0")
                    .join("0"),
                &data,
            );
        }
    }
    write(
        root.join("1").join(".zarray"),
        br#"{"zarr_format":2,"shape":[2,2,2,2,3],"chunks":[1,1,2,2,3],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}"#,
    );
    for time in 0..2u16 {
        for channel in 0..2u16 {
            let mut data = Vec::new();
            for z in 0..2u16 {
                for y in 0..2u16 {
                    for x in 0..3u16 {
                        let value = time * 10_000 + channel * 1000 + 20 * z + 5 * y + x;
                        data.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
            write(
                root.join("1")
                    .join(time.to_string())
                    .join(channel.to_string())
                    .join("0")
                    .join("0")
                    .join("0"),
                &data,
            );
        }
    }
}

fn write_v05(root: &Path) {
    write(
        root.join("zarr.json"),
        br#"{
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {
                "ome": {
                    "version": "0.5",
                    "multiscales": [{
                        "name": "fixture-v5",
                        "axes": [
                            {"name": "c", "type": "channel"},
                            {"name": "z", "type": "space", "unit": "micrometer"},
                            {"name": "y", "type": "space", "unit": "micrometer"},
                            {"name": "x", "type": "space", "unit": "micrometer"}
                        ],
                        "datasets": [
                            {"path": "0", "coordinateTransformations": [{"type": "scale", "scale": [1.0, 4.0, 0.5, 0.5]}]}
                        ]
                    }],
                    "omero": {
                        "channels": [
                            {"color": "FF0000", "window": {"min": 0, "max": 255, "start": 0, "end": 255}, "label": "RFP"}
                        ]
                    }
                }
            }
        }"#,
    );
    write(
        root.join("0").join("zarr.json"),
        br#"{
            "zarr_format": 3,
            "node_type": "array",
            "shape": [1, 2, 3, 4],
            "data_type": "uint8",
            "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [1, 2, 3, 4]}},
            "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
            "fill_value": 0,
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
            "attributes": {}
        }"#,
    );
    let mut data = Vec::new();
    for z in 0..2u8 {
        for y in 0..3u8 {
            for x in 0..4u8 {
                data.push(20 * z + 5 * y + x);
            }
        }
    }
    write(
        root.join("0")
            .join("c")
            .join("0")
            .join("0")
            .join("0")
            .join("0"),
        &data,
    );
}

fn write(path: PathBuf, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    fs::write(path, content).expect("the fixture files are written");
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
