use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use zarrs::array::builder::ArrayBuilderFillValue;
use zarrs::array::{Array, ArrayBuilder, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::GroupBuilder;
use zarrs::storage::WritableStorageTraits;

use crate::error::Error;
use crate::lineage::Lineage;

pub const VERSION: &str = "0.4.0";

pub struct Axes {
    pub spacing: [f32; 3],
    pub interval: f32,
}

impl Axes {
    pub fn new(spacing: [f32; 3], interval: f32) -> Self {
        assert!(
            spacing.iter().all(|step| step.is_finite() && *step > 0.0),
            "a graph carries a positive voxel size"
        );
        assert!(
            interval.is_finite() && interval > 0.0,
            "a graph carries a positive frame interval"
        );
        Self { spacing, interval }
    }
}

pub fn write(path: &Path, lineage: &Lineage, axes: &Axes) -> Result<(), Error> {
    if lineage.is_empty() {
        return Err(Error::Graph {
            detail: "the graph carries no node".to_string(),
        });
    }
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| Error::Write {
            detail: format!("{}: {error}", path.display()),
        })?;
    }
    std::fs::create_dir_all(path).map_err(|error| Error::Write {
        detail: format!("{}: {error}", path.display()),
    })?;
    let store = Arc::new(FilesystemStore::new(path).map_err(|error| Error::Write {
        detail: format!("{}: {error}", path.display()),
    })?);
    let limits = bounds(lineage);
    let attributes = json!({
        "geff": {
            "geff_version": VERSION,
            "directed": true,
            "axes": [
                {
                    "name": "t",
                    "type": "time",
                    "unit": "frame",
                    "min": limits.time[0],
                    "max": limits.time[1],
                },
                {
                    "name": "z",
                    "type": "space",
                    "unit": "micrometer",
                    "min": limits.space[2][0],
                    "max": limits.space[2][1],
                },
                {
                    "name": "y",
                    "type": "space",
                    "unit": "micrometer",
                    "min": limits.space[1][0],
                    "max": limits.space[1][1],
                },
                {
                    "name": "x",
                    "type": "space",
                    "unit": "micrometer",
                    "min": limits.space[0][0],
                    "max": limits.space[0][1],
                },
            ],
            "sphere": "radius",
            "node_props_metadata": {
                "t": property("t", "uint32", Some("frame")),
                "z": property("z", "float32", Some("micrometer")),
                "y": property("y", "float32", Some("micrometer")),
                "x": property("x", "float32", Some("micrometer")),
                "radius": property("radius", "float32", Some("micrometer")),
                "quality": property("quality", "float32", None),
            },
            "edge_props_metadata": {},
        },
        "grana": {
            "frame_interval": axes.interval,
            "voxel": axes.spacing,
        },
    });
    group(
        &store,
        "/",
        attributes.as_object().cloned().unwrap_or_default(),
    )?;
    group(&store, "/nodes", Map::new())?;
    group(&store, "/nodes/props", Map::new())?;
    group(&store, "/edges", Map::new())?;
    let count = lineage.len();
    let ids = lineage
        .detections()
        .iter()
        .map(|detection| detection.id())
        .collect::<Vec<_>>();
    let times = lineage
        .detections()
        .iter()
        .map(|detection| detection.time())
        .collect::<Vec<_>>();
    let values = [0, 1, 2].map(|axis| {
        lineage
            .detections()
            .iter()
            .map(|detection| detection.position()[axis])
            .collect::<Vec<_>>()
    });
    let radii = lineage
        .detections()
        .iter()
        .map(|detection| detection.radius())
        .collect::<Vec<_>>();
    let qualities = lineage
        .detections()
        .iter()
        .map(|detection| detection.quality())
        .collect::<Vec<_>>();
    array(
        &store,
        "/nodes/ids",
        vec![count as u64],
        data_type::uint64(),
        0u64,
        &ids,
    )?;
    array(
        &store,
        "/nodes/props/t/values",
        vec![count as u64],
        data_type::uint32(),
        0u32,
        &times,
    )?;
    for axis in 0..3 {
        array(
            &store,
            &format!("/nodes/props/{}/values", ["x", "y", "z"][axis]),
            vec![count as u64],
            data_type::float32(),
            0.0f32,
            &values[axis],
        )?;
    }
    array(
        &store,
        "/nodes/props/radius/values",
        vec![count as u64],
        data_type::float32(),
        0.0f32,
        &radii,
    )?;
    array(
        &store,
        "/nodes/props/quality/values",
        vec![count as u64],
        data_type::float32(),
        0.0f32,
        &qualities,
    )?;
    let edges = lineage
        .edges()
        .iter()
        .flat_map(|edge| [edge.source(), edge.target()])
        .collect::<Vec<_>>();
    let rows = lineage.edges().len() as u64;
    array(
        &store,
        "/edges/ids",
        vec![rows, 2],
        data_type::uint64(),
        0u64,
        &edges,
    )?;
    Ok(())
}

fn property(name: &str, dtype: &str, unit: Option<&str>) -> Value {
    let mut property = Map::new();
    property.insert("identifier".to_string(), json!(name));
    property.insert("dtype".to_string(), json!(dtype));
    property.insert("varlength".to_string(), json!(false));
    if let Some(unit) = unit {
        property.insert("unit".to_string(), json!(unit));
    }
    Value::Object(property)
}

struct Limits {
    time: [f32; 2],
    space: [[f32; 2]; 3],
}

fn bounds(lineage: &Lineage) -> Limits {
    let mut limits = Limits {
        time: [f32::MAX, f32::MIN],
        space: [[f32::MAX, f32::MIN]; 3],
    };
    for detection in lineage.detections() {
        let time = detection.time() as f32;
        limits.time[0] = limits.time[0].min(time);
        limits.time[1] = limits.time[1].max(time);
        for axis in 0..3 {
            let value = detection.position()[axis];
            limits.space[axis][0] = limits.space[axis][0].min(value);
            limits.space[axis][1] = limits.space[axis][1].max(value);
        }
    }
    limits
}

fn group(
    store: &Arc<FilesystemStore>,
    path: &str,
    attributes: Map<String, Value>,
) -> Result<(), Error> {
    let mut builder = GroupBuilder::new();
    builder.attributes(attributes);
    let group = builder
        .build(store.clone(), path)
        .map_err(|error| Error::Write {
            detail: error.to_string(),
        })?;
    group.store_metadata().map_err(|error| Error::Write {
        detail: error.to_string(),
    })
}

fn array<T: zarrs::array::Element + Into<ArrayBuilderFillValue> + Copy>(
    store: &Arc<FilesystemStore>,
    path: &str,
    shape: Vec<u64>,
    dtype: zarrs::array::DataType,
    fill: T,
    values: &[T],
) -> Result<(), Error> {
    let chunk = shape
        .iter()
        .map(|dimension| (*dimension).max(1))
        .collect::<Vec<u64>>();
    let array = ArrayBuilder::new(shape, chunk, dtype, fill)
        .build(store.clone(), path)
        .map_err(|error| Error::Write {
            detail: error.to_string(),
        })?;
    array.store_metadata().map_err(|error| Error::Write {
        detail: error.to_string(),
    })?;
    store_chunk(&array, values)
}

fn store_chunk<S, T>(array: &Array<S>, values: &[T]) -> Result<(), Error>
where
    S: ?Sized + WritableStorageTraits + 'static,
    T: zarrs::array::Element,
{
    if values.is_empty() {
        return Ok(());
    }
    let indices = vec![0u64; array.chunk_grid_shape().len()];
    array
        .store_chunk(&indices, values)
        .map_err(|error| Error::Write {
            detail: error.to_string(),
        })
}
