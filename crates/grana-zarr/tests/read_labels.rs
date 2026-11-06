use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Extent, Spacing};
use grana_zarr::{LabelSpec, read_labels, write_labels};

const WIDTH: u32 = 6;
const HEIGHT: u32 = 5;
const DEPTH: u32 = 4;

#[test]
fn reads_a_written_label_image() {
    let temp = TempDir::new("grana-read-label");
    let root = temp.path().join("image.zarr");
    write_image(&root);
    let mut fill =
        |_level: usize, _time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    for x in 0..extent[0] {
                        let at = (x + extent[0] * (y + extent[1] * z)) as usize;
                        let cell = [origin[0] + x, origin[1] + y, origin[2] + z];
                        data[at] = expected(cell) as u16;
                    }
                }
            }
            Ok(())
        };
    let mut progress = |_done: usize, _total: usize| {};
    let spec = LabelSpec {
        name: "cells",
        frames: &[0, 1],
        labels: 2,
        color: [0.2, 0.4, 0.9],
        opacity: 0.8,
        properties: &[(1, "first".to_string()), (2, "second".to_string())],
        fill: &mut fill,
        progress: &mut progress,
    };
    write_labels(&root, spec).expect("the label image is written");

    let labels = read_labels(&root).expect("the label images read");
    assert_eq!(labels.names().collect::<Vec<_>>(), vec!["cells"]);
    let label = labels.get("cells").expect("the label image exists");
    assert_eq!(label.frames(), 2);
    assert_eq!(label.level_count(), 1);
    let level = label.level(0).expect("the level exists");
    assert_eq!(level.extent(), Extent::new(WIDTH, HEIGHT, DEPTH));
    assert_eq!(level.spacing(), Spacing::new(0.5, 0.25, 2.0));

    let data = label
        .read(1, 0, &[0..WIDTH, 0..HEIGHT, 0..DEPTH], [0, 0, 0])
        .expect("the first frame reads");
    assert_eq!(data.len(), (WIDTH * HEIGHT * DEPTH) as usize);
    for z in 0..DEPTH {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let at = (x + WIDTH * (y + HEIGHT * z)) as usize;
                assert_eq!(data[at], expected([x, y, z]), "voxel {x},{y},{z}");
            }
        }
    }

    let shifted = label
        .read(0, 0, &[0..WIDTH, 0..HEIGHT, 0..DEPTH], [1, 0, 0])
        .expect("the shifted frame reads");
    for z in 0..DEPTH {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let at = (x + WIDTH * (y + HEIGHT * z)) as usize;
                let source = if x + 1 < WIDTH {
                    expected([x + 1, y, z])
                } else {
                    0
                };
                assert_eq!(shifted[at], source, "voxel {x},{y},{z}");
            }
        }
    }
}

fn expected(cell: [u32; 3]) -> u32 {
    if cell[1] == 0 {
        0
    } else if cell[0] < 3 {
        1
    } else {
        2
    }
}

fn write_image(root: &Path) {
    write(
        root.join("zarr.json"),
        serde_json::json!({
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {
                "ome": {
                    "version": "0.5",
                    "multiscales": [{
                        "name": "fixture",
                        "axes": [
                            {"name": "t", "type": "time", "unit": "second"},
                            {"name": "z", "type": "space", "unit": "micrometer"},
                            {"name": "y", "type": "space", "unit": "micrometer"},
                            {"name": "x", "type": "space", "unit": "micrometer"}
                        ],
                        "datasets": [{
                            "path": "0",
                            "coordinateTransformations": [{
                                "type": "scale",
                                "scale": [1.0, 2.0, 0.25, 0.5],
                            }],
                        }],
                    }],
                    "omero": {
                        "channels": [{
                            "color": "00FF00",
                            "window": {"min": 0, "max": 65535, "start": 0, "end": 65535},
                            "label": "GFP",
                        }],
                    },
                },
            },
        })
        .to_string()
        .as_bytes(),
    );
    write(
        root.join("0").join("zarr.json"),
        serde_json::json!({
            "zarr_format": 3,
            "node_type": "array",
            "shape": [2, DEPTH, HEIGHT, WIDTH],
            "data_type": "uint16",
            "chunk_grid": {
                "name": "regular",
                "configuration": {"chunk_shape": [1, DEPTH, HEIGHT, WIDTH]},
            },
            "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
            "fill_value": 0,
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
            "attributes": {},
        })
        .to_string()
        .as_bytes(),
    );
    for frame in 0..2u32 {
        let mut data = Vec::with_capacity((WIDTH * HEIGHT * DEPTH * 2) as usize);
        for z in 0..DEPTH {
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let value = (frame * 1000 + z * 100 + y * 10 + x) as u16;
                    data.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        write(
            root.join("0")
                .join(frame.to_string())
                .join("0")
                .join("0")
                .join("0"),
            &data,
        );
    }
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
