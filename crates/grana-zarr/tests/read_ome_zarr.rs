use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Extent, Intensity, Spacing, VoxelData};
use grana_zarr::Image;

#[test]
fn reads_ome_zarr_v04_and_v05() {
    let temp = TempDir::new("grana-zarr");
    let v4 = temp.path().join("v04.zarr");
    write_v04(&v4);
    let v5 = temp.path().join("v05.zarr");
    write_v05(&v5);

    let image = Image::open(&v4).expect("the v0.4 dataset opens");
    assert_eq!(image.name(), "fixture");
    assert_eq!(image.channels().len(), 2);
    assert_eq!(image.channels()[0].name, "DAPI");
    assert_eq!(image.channels()[0].tint, [0.0, 0.0, 1.0]);
    assert_eq!(image.channels()[0].window, Some([0.0, 65535.0]));
    let levels = image.levels().collect::<Vec<_>>();
    assert_eq!(levels.len(), 2);
    assert_eq!(levels[0].extent, Extent::new(5, 4, 3));
    assert_eq!(levels[0].spacing, Spacing::new(0.5, 0.5, 4.0));
    assert_eq!(levels[1].extent, Extent::new(3, 2, 2));
    assert_eq!(levels[1].spacing, Spacing::new(1.0, 1.0, 4.0));

    let volume = image.read(0).expect("the v0.4 volume reads");
    assert_eq!(volume.extent(), Extent::new(5, 4, 3));
    assert_eq!(volume.spacing(), Spacing::new(0.5, 0.5, 4.0));
    assert_eq!(volume.channel_count(), 2);
    assert_eq!(volume.channels()[0].name(), "DAPI");
    assert_eq!(volume.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(volume.channels()[0].window(), Intensity::new(0.0, 65535.0));
    assert_eq!(volume.channels()[0].intensity().max(), 234.0);
    let first = volume.channels()[0].levels()[0].data();
    let VoxelData::U16(data) = first else {
        panic!("the fixture stores uint16 voxels");
    };
    for z in 0..3 {
        for y in 0..4 {
            for x in 0..5 {
                let index = (z * 4 * 5 + y * 5 + x) as usize;
                assert_eq!(data[index], (100 * z + 10 * y + x) as u16);
            }
        }
    }
    let VoxelData::U16(second) = volume.channels()[1].levels()[0].data() else {
        panic!("the fixture stores uint16 voxels");
    };
    assert_eq!(second[0], 1000);

    let image = Image::open(&v5).expect("the v0.5 dataset opens");
    assert_eq!(image.name(), "fixture-v5");
    assert_eq!(image.channels().len(), 1);
    assert_eq!(image.channels()[0].name, "RFP");
    assert_eq!(image.channels()[0].tint, [1.0, 0.0, 0.0]);
    let volume = image.read(0).expect("the v0.5 volume reads");
    assert_eq!(volume.extent(), Extent::new(4, 3, 2));
    assert_eq!(volume.spacing(), Spacing::new(0.5, 0.5, 4.0));
    let VoxelData::U8(data) = volume.channels()[0].levels()[0].data() else {
        panic!("the fixture stores uint8 voxels");
    };
    for z in 0..2 {
        for y in 0..3 {
            for x in 0..4 {
                let index = (z * 3 * 4 + y * 4 + x) as usize;
                assert_eq!(data[index], (20 * z + 5 * y + x) as u8);
            }
        }
    }
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
                    {"name": "c", "type": "channel"},
                    {"name": "z", "type": "space", "unit": "micrometer"},
                    {"name": "y", "type": "space", "unit": "micrometer"},
                    {"name": "x", "type": "space", "unit": "micrometer"}
                ],
                "datasets": [
                    {"path": "0", "coordinateTransformations": [{"type": "scale", "scale": [1.0, 4.0, 0.5, 0.5]}]},
                    {"path": "1", "coordinateTransformations": [{"type": "scale", "scale": [1.0, 4.0, 1.0, 1.0]}]}
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
        br#"{"zarr_format":2,"shape":[2,3,4,5],"chunks":[1,3,4,5],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}"#,
    );
    for channel in 0..2u16 {
        let mut data = Vec::new();
        for z in 0..3u16 {
            for y in 0..4u16 {
                for x in 0..5u16 {
                    data.extend_from_slice(&(channel * 1000 + 100 * z + 10 * y + x).to_le_bytes());
                }
            }
        }
        write(
            root.join("0")
                .join(channel.to_string())
                .join("0")
                .join("0")
                .join("0"),
            &data,
        );
    }
    write(
        root.join("1").join(".zarray"),
        br#"{"zarr_format":2,"shape":[2,2,2,3],"chunks":[1,2,2,3],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}"#,
    );
    for channel in 0..2u16 {
        let mut data = Vec::new();
        for z in 0..2u16 {
            for y in 0..3u16 {
                for x in 0..4u16 {
                    data.extend_from_slice(&(channel * 1000 + 20 * z + 5 * y + x).to_le_bytes());
                }
            }
        }
        write(
            root.join("1")
                .join(channel.to_string())
                .join("0")
                .join("0")
                .join("0"),
            &data,
        );
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
