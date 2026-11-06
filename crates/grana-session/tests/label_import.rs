use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use grana_scene::ObjectFrame;
use grana_session::{Command, Session};
use grana_zarr::{LabelSpec, write_labels};

const WIDTH: u32 = 6;
const HEIGHT: u32 = 5;
const DEPTH: u32 = 4;

#[test]
fn imports_label_voxels_as_a_measured_object_set() {
    let temp = TempDir::new("grana-label-import");
    let root = temp.path().join("image.zarr");
    write_image(&root);
    let mut session = Session::new();
    session.begin_load(root);
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the dataset never opened");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(session.label_images(), &["cells".to_string()]);
    session.execute(Command::ImportLabels {
        label: "cells".to_string(),
    });
    while session
        .progress()
        .iter()
        .any(|progress| matches!(progress.job, grana_scene::Job::Import(_)))
        || session.scene().is_some_and(|scene| {
            scene
                .objects
                .iter()
                .any(|set| set.provenance().is_imported() && set.detected() == 0)
        })
    {
        assert!(Instant::now() < deadline, "the import never finished");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let scene = session.scene().expect("the dataset is open");
    let set = scene
        .objects
        .iter()
        .find(|set| set.provenance().is_imported())
        .expect("the label image became an object set");
    assert_eq!(set.provenance().label(), Some("cells"));
    assert_eq!(set.name(), "Labels cells");
    let frame = set.frame(0).expect("the import filled the frame");
    let object = frame.intensities().object(0, 0);
    let ObjectFrame::Surfaces(surfaces) = frame else {
        panic!("the labels form surfaces");
    };
    let statistics = surfaces.object(0).expect("the surface exists");
    assert_eq!(frame.object_count(), 1);
    assert!((object.mean() - 172.0).abs() < 1.0e-3);
    assert_eq!(statistics.voxel_count(), 18);
    assert!(statistics.volume() > 0.0);
    assert!(surfaces.geometry().is_ok());
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
                            {"name": "z", "type": "space", "unit": "micrometer"},
                            {"name": "y", "type": "space", "unit": "micrometer"},
                            {"name": "x", "type": "space", "unit": "micrometer"}
                        ],
                        "datasets": [{
                            "path": "0",
                            "coordinateTransformations": [{
                                "type": "scale",
                                "scale": [1.0, 1.0, 1.0],
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
            "shape": [DEPTH, HEIGHT, WIDTH],
            "data_type": "uint16",
            "chunk_grid": {
                "name": "regular",
                "configuration": {"chunk_shape": [DEPTH, HEIGHT, WIDTH]},
            },
            "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
            "fill_value": 0,
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
            "attributes": {},
        })
        .to_string()
        .as_bytes(),
    );
    let mut data = Vec::with_capacity((WIDTH * HEIGHT * DEPTH * 2) as usize);
    for z in 0..DEPTH {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let value = (100 * z + 10 * y + x) as u16;
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    write(
        root.join("0").join("c").join("0").join("0").join("0"),
        &data,
    );
    let mut fill =
        |_level: usize, _time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    for x in 0..extent[0] {
                        let at = (x + extent[0] * (y + extent[1] * z)) as usize;
                        let cell = [origin[0] + x, origin[1] + y, origin[2] + z];
                        let solid = (1..4).contains(&cell[0])
                            && (1..4).contains(&cell[1])
                            && (1..3).contains(&cell[2]);
                        data[at] = if solid { 1 } else { 0 };
                    }
                }
            }
            Ok(())
        };
    let mut progress = |_done: usize, _total: usize| {};
    let spec = LabelSpec {
        name: "cells",
        frames: &[0],
        labels: 1,
        color: [0.2, 0.4, 0.9],
        opacity: 0.8,
        properties: &[(1, "cell".to_string())],
        fill: &mut fill,
        progress: &mut progress,
    };
    write_labels(root, spec).expect("the label image is written");
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
