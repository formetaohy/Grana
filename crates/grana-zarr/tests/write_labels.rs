use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use grana_zarr::{LabelSpec, write_labels};
use zarrs::array::{Array, ArraySubset};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::Group;

const FRAMES: u32 = 2;
const LEVELS: [([u32; 3], [f32; 3]); 2] =
    [([5, 4, 3], [0.5, 0.5, 4.0]), ([3, 2, 2], [1.0, 1.0, 4.0])];

#[test]
fn writes_an_ome_zarr_label_image() {
    let temp = TempDir::new("grana-label");
    let root = temp.path().join("image.zarr");
    write_image(&root);
    let mut fill =
        |level: usize, _time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
            assert!(level < LEVELS.len());
            let [x, y, z] = origin;
            let [nx, ny, nz] = extent;
            for k in 0..nz {
                for j in 0..ny {
                    for i in 0..nx {
                        let value = if x + i < 2 { 1 } else { 2 };
                        data[(i + nx * (j + ny * k)) as usize] = value;
                        assert!(y + j < 4 && z + k < 3);
                    }
                }
            }
            Ok(())
        };
    let mut progress = |_done: usize, _total: usize| {};
    let properties = vec![(1, "first".to_string()), (2, "second".to_string())];
    let spec = LabelSpec {
        name: "grana-test",
        frames: &[0, 1],
        labels: 2,
        color: [0.25, 0.5, 1.0],
        opacity: 0.5,
        properties: &properties,
        fill: &mut fill,
        progress: &mut progress,
    };
    let written = write_labels(&root, spec).expect("the label image is written");
    assert_eq!(written.name(), "grana-test");
    assert_eq!(written.labels(), 2);
    assert_eq!(written.frames(), 2);
    assert!(written.path().join("zarr.json").is_file());

    let store = Arc::new(FilesystemStore::new(&root).expect("the store opens"));
    let labels = Group::open(store.clone(), "/labels").expect("the labels group exists");
    let ome = labels
        .attributes()
        .get("ome")
        .and_then(|value| value.as_object())
        .expect("the labels group carries ome metadata");
    assert_eq!(ome["version"], "0.5");
    assert!(
        ome["labels"]
            .as_array()
            .expect("the labels list is an array")
            .iter()
            .any(|value| value.as_str() == Some("grana-test"))
    );

    let label = Group::open(store.clone(), "/labels/grana-test").expect("the label group exists");
    let ome = label
        .attributes()
        .get("ome")
        .and_then(|value| value.as_object())
        .expect("the label group carries ome metadata");
    let multiscales = ome["multiscales"]
        .as_array()
        .expect("the label multiscales is an array");
    assert_eq!(multiscales.len(), 1);
    assert_eq!(
        multiscales[0]["datasets"].as_array().map(Vec::len),
        Some(LEVELS.len())
    );
    let image_label = ome["image-label"]
        .as_object()
        .expect("the label group carries image-label metadata");
    assert_eq!(image_label["colors"].as_array().map(Vec::len), Some(2));
    assert_eq!(image_label["properties"].as_array().map(Vec::len), Some(2));

    let array = Array::open(store.clone(), "/labels/grana-test/0").expect("the level opens");
    assert_eq!(array.shape(), &[2, 1, 3, 4, 5]);
    let current: Vec<u16> = array
        .retrieve_array_subset(&ArraySubset::new_with_ranges(&[
            1..2,
            0..1,
            0..3,
            0..4,
            0..5,
        ]))
        .expect("the exported frame reads");
    assert_eq!(current[0], 1);
    assert_eq!(current[2], 2);
    assert_eq!(current[7], 2);
    let first: Vec<u16> = array
        .retrieve_array_subset(&ArraySubset::new_with_ranges(&[
            0..1,
            0..1,
            0..3,
            0..4,
            0..5,
        ]))
        .expect("the first exported frame reads");
    assert_eq!(first[0], 1, "every exported frame carries its labels");
    assert_eq!(first[2], 2);

    let coarse =
        Array::open(store.clone(), "/labels/grana-test/1").expect("the coarse level opens");
    assert_eq!(coarse.shape(), &[2, 1, 2, 2, 3]);

    let mut fill =
        |_level: usize, _time: u32, _origin: [u32; 3], _extent: [u32; 3], data: &mut [u16]| {
            data.fill(1);
            Ok(())
        };
    let mut progress = |_done: usize, _total: usize| {};
    let spec = LabelSpec {
        name: "grana-test",
        frames: &[1],
        labels: 2,
        color: [0.25, 0.5, 1.0],
        opacity: 0.5,
        properties: &properties,
        fill: &mut fill,
        progress: &mut progress,
    };
    write_labels(&root, spec).expect("the label image is replaced");
    let labels = Group::open(store.clone(), "/labels").expect("the labels group exists");
    let names = labels.attributes()["ome"]["labels"]
        .as_array()
        .expect("the labels list is an array");
    assert_eq!(
        names
            .iter()
            .filter(|value| value.as_str() == Some("grana-test"))
            .count(),
        1,
        "the label image is registered once"
    );
    let array = Array::open(store.clone(), "/labels/grana-test/0").expect("the level opens");
    let empty: Vec<u16> = array
        .retrieve_array_subset(&ArraySubset::new_with_ranges(&[
            0..1,
            0..1,
            0..3,
            0..4,
            0..5,
        ]))
        .expect("the replaced frame reads");
    assert!(
        empty.iter().all(|value| *value == 0),
        "a replaced label image drops the frames it no longer exports"
    );
    let filled: Vec<u16> = array
        .retrieve_array_subset(&ArraySubset::new_with_ranges(&[
            1..2,
            0..1,
            0..3,
            0..4,
            0..5,
        ]))
        .expect("the exported frame reads");
    assert!(filled.iter().all(|value| *value == 1));
}

fn write_image(root: &Path) {
    let datasets = LEVELS
        .iter()
        .enumerate()
        .map(|(index, (_, scale))| {
            serde_json::json!({
                "path": index.to_string(),
                "coordinateTransformations": [{
                    "type": "scale",
                    "scale": [1.0, 1.0, scale[2], scale[1], scale[0]],
                }],
            })
        })
        .collect::<Vec<_>>();
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
                            {"name": "c", "type": "channel"},
                            {"name": "z", "type": "space", "unit": "micrometer"},
                            {"name": "y", "type": "space", "unit": "micrometer"},
                            {"name": "x", "type": "space", "unit": "micrometer"}
                        ],
                        "datasets": datasets,
                    }],
                    "omero": {
                        "channels": [
                            {"color": "0000FF", "window": {"min": 0, "max": 65535, "start": 0, "end": 65535}, "label": "DAPI"}
                        ]
                    }
                }
            }
        })
        .to_string()
        .as_bytes(),
    );
    for (level, (extent, _)) in LEVELS.iter().enumerate() {
        write(
            root.join(level.to_string()).join("zarr.json"),
            serde_json::json!({
                "zarr_format": 3,
                "node_type": "array",
                "shape": [FRAMES, 1, extent[2], extent[1], extent[0]],
                "data_type": "uint16",
                "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [1, 1, extent[2], extent[1], extent[0]]}},
                "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
                "fill_value": 0,
                "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
                "attributes": {}
            })
            .to_string()
            .as_bytes(),
        );
        let voxels = (extent[0] * extent[1] * extent[2]) as usize;
        for frame in 0..FRAMES {
            let mut data = Vec::with_capacity(voxels * 2);
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    for x in 0..extent[0] {
                        let value = (frame * 100 + z * 10 + y + x) as u16;
                        data.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
            write(
                root.join(level.to_string())
                    .join(frame.to_string())
                    .join("0")
                    .join("0")
                    .join("0")
                    .join("0"),
                &data,
            );
        }
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
