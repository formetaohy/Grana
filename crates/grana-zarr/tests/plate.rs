use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Extent, Spacing, VoxelData};
use grana_zarr::Image;

#[test]
fn reads_the_fields_of_a_plate_in_row_major_order() {
    let temp = TempDir::new("grana-zarr-plate");
    let path = temp.path().join("plate.zarr");
    write_plate(&path);
    let root = grana_zarr::Root::local(&path).expect("the plate root opens");

    let plate = grana_zarr::open_plate(&root).expect("the plate opens");
    assert_eq!(plate.name(), "plate");
    assert_eq!(plate.len(), 2);
    assert_eq!(plate.fields()[0].path(), "/A/1/0");
    assert_eq!(plate.fields()[0].name(), "A1/0");
    assert_eq!(plate.fields()[1].path(), "/A/2/0");
    assert_eq!(plate.fields()[1].name(), "A2/0");

    let first = Image::open_group(&path, plate.fields()[0].path()).expect("the first field opens");
    assert_eq!(first.name(), "field one");
    assert_eq!(first.pyramid().level(0).extent(), Extent::new(4, 3, 2));
    assert_eq!(
        first.pyramid().level(0).spacing(),
        Spacing::new(0.5, 0.5, 4.0)
    );
    assert_eq!(voxel(&first, 2, 1, 0), 17);

    let second =
        Image::open_group(&path, plate.fields()[1].path()).expect("the second field opens");
    assert_eq!(second.name(), "field two");
    assert_eq!(voxel(&second, 2, 1, 0), 27);

    assert_eq!(
        grana_zarr::group_kind(&root).expect("the plate kind reads"),
        grana_zarr::GroupKind::Plate
    );
    let well = grana_zarr::Root::local(&path.join("A").join("1")).expect("the well root opens");
    assert_eq!(
        grana_zarr::group_kind(&well).expect("the well kind reads"),
        grana_zarr::GroupKind::Well
    );
    let field =
        grana_zarr::Root::local(&path.join("A").join("1").join("0")).expect("the field root opens");
    assert_eq!(
        grana_zarr::group_kind(&field).expect("the field kind reads"),
        grana_zarr::GroupKind::Image
    );
}

fn voxel(image: &Image, x: u32, y: u32, z: u32) -> u8 {
    let region = image
        .read_region(0, 0, 0, x..x + 1, y..y + 1, z..z + 1)
        .expect("the field reads");
    let VoxelData::U8(values) = &region else {
        panic!("the field stores uint8 voxels");
    };
    values[0]
}

fn write_plate(root: &Path) {
    write(
        &root.join("zarr.json"),
        br#"{
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {
                "ome": {
                    "version": "0.5",
                    "plate": {
                        "columns": [{"name": "1"}, {"name": "2"}],
                        "rows": [{"name": "A"}],
                        "wells": [
                            {"path": "A/2", "rowIndex": 0, "columnIndex": 1},
                            {"path": "A/1", "rowIndex": 0, "columnIndex": 0}
                        ]
                    }
                }
            }
        }"#,
    );
    for (column, name, base) in [(1u8, "field one", 10u8), (2, "field two", 20)] {
        let well = root.join("A").join(column.to_string());
        write(
            &well.join("zarr.json"),
            br#"{
                "zarr_format": 3,
                "node_type": "group",
                "attributes": {
                    "ome": {"version": "0.5", "well": {"images": [{"path": "0"}]}}
                }
            }"#,
        );
        write_field(&well.join("0"), name, base);
    }
}

fn write_field(field: &Path, name: &str, base: u8) {
    let attributes = format!(
        r#"{{
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {{
                "ome": {{
                    "version": "0.5",
                    "multiscales": [{{
                        "name": "{name}",
                        "axes": [
                            {{"name": "c", "type": "channel"}},
                            {{"name": "z", "type": "space", "unit": "micrometer"}},
                            {{"name": "y", "type": "space", "unit": "micrometer"}},
                            {{"name": "x", "type": "space", "unit": "micrometer"}}
                        ],
                        "datasets": [
                            {{"path": "0", "coordinateTransformations": [{{"type": "scale", "scale": [1.0, 4.0, 0.5, 0.5]}}]}}
                        ]
                    }}],
                    "omero": {{
                        "channels": [
                            {{"color": "FF0000", "window": {{"min": 0, "max": 255, "start": 0, "end": 255}}, "label": "RFP"}}
                        ]
                    }}
                }}
            }}
        }}"#
    );
    write(&field.join("zarr.json"), attributes.as_bytes());
    let array = r#"{
            "zarr_format": 3,
            "node_type": "array",
            "shape": [1, 2, 3, 4],
            "data_type": "uint8",
            "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [1, 2, 3, 4]}},
            "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
            "fill_value": 0,
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
            "attributes": {}
        }"#;
    write(&field.join("0").join("zarr.json"), array.as_bytes());
    let mut chunk = Vec::new();
    for z in 0..2u32 {
        for y in 0..3u32 {
            for x in 0..4u32 {
                chunk.push(base + (10 * z + 5 * y + x) as u8);
            }
        }
    }
    write(
        &field
            .join("0")
            .join("c")
            .join("0")
            .join("0")
            .join("0")
            .join("0"),
        &chunk,
    );
}

fn write(path: &Path, content: &[u8]) {
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
