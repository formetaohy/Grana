use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use zarrs::array::builder::ArrayBuilderFillValue;
use zarrs::array::{Array, ArrayBuilder, ArraySubset, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::{Group, GroupBuilder};
use zarrs::storage::WritableStorageTraits;

use crate::error::Error;
use crate::lineage::{Detection, Edge, Lineage};

pub const VERSION: &str = "0.4.0";

const CONTINUE: u8 = 0;
const GAP: u8 = 1;
const DIVISION: u8 = 2;
const MERGE: u8 = 3;

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
            "edge_props_metadata": {
                "kind": property("kind", "uint8", None),
            },
            "extra": {
                "grana": {
                    "frame_interval": axes.interval,
                    "voxel": axes.spacing,
                },
            },
        },
    });
    group(
        &store,
        "/",
        attributes.as_object().cloned().unwrap_or_default(),
    )?;
    group(&store, "/nodes", Map::new())?;
    group(&store, "/nodes/props", Map::new())?;
    for name in ["t", "x", "y", "z", "radius", "quality"] {
        group(&store, &format!("/nodes/props/{name}"), Map::new())?;
    }
    group(&store, "/edges", Map::new())?;
    group(&store, "/edges/props", Map::new())?;
    group(&store, "/edges/props/kind", Map::new())?;
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
    let kind = kinds(lineage);
    let rows = lineage.edges().len() as u64;
    array(
        &store,
        "/edges/ids",
        vec![rows, 2],
        data_type::uint64(),
        0u64,
        &edges,
    )?;
    array(
        &store,
        "/edges/props/kind/values",
        vec![rows],
        data_type::uint8(),
        0u8,
        &kind,
    )
}

pub fn read(path: &Path, default_radius: f32) -> Result<Lineage, Error> {
    assert!(
        default_radius.is_finite() && default_radius > 0.0,
        "a graph read names a positive fallback radius"
    );
    let store = Arc::new(FilesystemStore::new(path).map_err(|error| Error::Read {
        detail: format!("{}: {error}", path.display()),
    })?);
    let graph = graph(&store)?;
    let attributes = graph.attributes();
    let metadata = attributes
        .get("geff")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Format {
            detail: "the group carries no geff metadata".to_string(),
        })?;
    match metadata.get("directed") {
        Some(Value::Bool(true)) => {}
        _ => {
            return Err(Error::Format {
                detail: "the graph is not directed".to_string(),
            });
        }
    }
    let names = names(metadata)?;
    let ids = read_ids(&store, &graph, "nodes/ids")?;
    if ids.is_empty() {
        return Err(Error::Format {
            detail: "the graph holds no node".to_string(),
        });
    }
    let mut slots = HashMap::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        if slots.insert(*id, slot).is_some() {
            return Err(Error::Format {
                detail: format!("the node {id} appears twice"),
            });
        }
    }
    let times = read_numbers(
        &store,
        &graph,
        &format!("nodes/props/{}/values", names.time),
    )?
    .into_iter()
    .map(time)
    .collect::<Result<Vec<_>, _>>()?;
    let positions = [
        read_axis(&store, &graph, names.space[0].as_deref(), ids.len())?,
        read_axis(&store, &graph, names.space[1].as_deref(), ids.len())?,
        read_axis(&store, &graph, names.space[2].as_deref(), ids.len())?,
    ];
    if times.len() != ids.len() || positions.iter().any(|values| values.len() != ids.len()) {
        return Err(Error::Format {
            detail: "the node properties do not cover every node".to_string(),
        });
    }
    let radii = if declared(metadata, &names.sphere) {
        Some(read_numbers(
            &store,
            &graph,
            &format!("nodes/props/{}/values", names.sphere),
        )?)
    } else {
        None
    };
    if radii
        .as_ref()
        .is_some_and(|values| values.len() != ids.len())
    {
        return Err(Error::Format {
            detail: "the radii do not cover every node".to_string(),
        });
    }
    let missing = read_missing(&store, &graph, &names.sphere)?;
    if !missing.is_empty() && missing.len() != ids.len() {
        return Err(Error::Format {
            detail: "the missing radii do not cover every node".to_string(),
        });
    }
    let qualities = if declared(metadata, "quality") {
        Some(read_numbers(&store, &graph, "nodes/props/quality/values")?)
    } else {
        None
    };
    if qualities
        .as_ref()
        .is_some_and(|values| values.len() != ids.len())
    {
        return Err(Error::Format {
            detail: "the qualities do not cover every node".to_string(),
        });
    }
    let mut detections = Vec::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        let radius = radii
            .as_ref()
            .filter(|_| !missing.get(slot).copied().unwrap_or(false))
            .map_or(default_radius, |values| values[slot] as f32);
        if !radius.is_finite() || radius <= 0.0 {
            return Err(Error::Format {
                detail: format!("the node {id} carries a degenerate radius"),
            });
        }
        let quality = qualities.as_ref().map_or(0.0, |values| values[slot] as f32);
        if !quality.is_finite() {
            return Err(Error::Format {
                detail: format!("the node {id} carries a degenerate quality"),
            });
        }
        detections.push(Detection::new(
            *id,
            times[slot],
            [
                positions[0][slot] as f32,
                positions[1][slot] as f32,
                positions[2][slot] as f32,
            ],
            radius,
            quality,
        ));
    }
    let links = read_ids(&store, &graph, "edges/ids")?;
    if links.len() % 2 != 0 {
        return Err(Error::Format {
            detail: "the edge array holds an odd number of ids".to_string(),
        });
    }
    let edges = links
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Edge::new(pair[0], pair[1]))
        .collect::<Vec<_>>();
    Lineage::new(detections, edges)
}

struct Names {
    time: String,
    space: [Option<String>; 3],
    sphere: String,
}

fn names(metadata: &Map<String, Value>) -> Result<Names, Error> {
    let sphere = metadata
        .get("sphere")
        .and_then(Value::as_str)
        .unwrap_or("radius")
        .to_string();
    let Some(list) = metadata.get("axes").and_then(Value::as_array) else {
        return Ok(Names {
            time: "t".to_string(),
            space: [
                Some("x".to_string()),
                Some("y".to_string()),
                Some("z".to_string()),
            ],
            sphere,
        });
    };
    let mut time = None;
    let mut time_unit = None;
    let mut space = Vec::new();
    for axis in list {
        let axis = axis.as_object().ok_or_else(|| Error::Format {
            detail: "an axis is not an object".to_string(),
        })?;
        let name = axis
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Format {
                detail: "an axis carries no name".to_string(),
            })?
            .to_string();
        let unit = axis.get("unit").and_then(Value::as_str);
        match axis.get("type").and_then(Value::as_str) {
            Some("time") => {
                time = Some(name);
                time_unit = unit;
            }
            Some("space") => space.push((name, unit)),
            _ => {}
        }
    }
    let time = time.ok_or_else(|| Error::Format {
        detail: "the axes name no time axis".to_string(),
    })?;
    if !frame_unit(time_unit) {
        return Err(Error::Format {
            detail: "the time axis is not measured in frames".to_string(),
        });
    }
    let named = |name: &str| {
        space
            .iter()
            .find(|(axis, _)| axis.eq_ignore_ascii_case(name))
            .cloned()
    };
    let picked = match (named("x"), named("y"), named("z")) {
        (Some(x), Some(y), Some(z)) => [Some(x), Some(y), Some(z)],
        _ if space.len() >= 3 => {
            let mut reversed = space.iter().rev().cloned();
            let z = reversed.next();
            let y = reversed.next();
            let x = reversed.next();
            [x, y, z]
        }
        _ if space.len() == 2 => {
            let x = named("x").or_else(|| space.get(1).cloned());
            let y = named("y").or_else(|| space.first().cloned());
            [x, y, None]
        }
        _ => {
            return Err(Error::Format {
                detail: "the axes name no two spatial dimensions".to_string(),
            });
        }
    };
    for (_, unit) in picked.iter().flatten() {
        if !length_unit(*unit) {
            return Err(Error::Format {
                detail: "a spatial axis is not measured in micrometers".to_string(),
            });
        }
    }
    Ok(Names {
        time,
        space: [
            picked[0].as_ref().map(|(name, _)| name.clone()),
            picked[1].as_ref().map(|(name, _)| name.clone()),
            picked[2].as_ref().map(|(name, _)| name.clone()),
        ],
        sphere,
    })
}

fn frame_unit(unit: Option<&str>) -> bool {
    unit.is_none_or(|unit| {
        matches!(
            unit.to_ascii_lowercase().as_str(),
            "frame" | "frames" | "timepoint" | "timepoints"
        )
    })
}

fn length_unit(unit: Option<&str>) -> bool {
    unit.is_none_or(|unit| {
        matches!(
            unit.to_ascii_lowercase().as_str(),
            "micrometer" | "micrometers" | "um" | "µm" | "μm"
        )
    })
}

fn time(value: f64) -> Result<u32, Error> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(Error::Format {
            detail: format!("the node time {value} is not a frame"),
        });
    }
    Ok(value.round() as u32)
}

fn declared(metadata: &Map<String, Value>, name: &str) -> bool {
    metadata
        .get("node_props_metadata")
        .and_then(Value::as_object)
        .is_some_and(|props| props.contains_key(name))
}

fn read_axis(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: Option<&str>,
    length: usize,
) -> Result<Vec<f64>, Error> {
    let Some(name) = name else {
        return Ok(vec![0.0; length]);
    };
    read_numbers(store, graph, &format!("nodes/props/{name}/values"))
}

fn read_missing(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: &str,
) -> Result<Vec<bool>, Error> {
    let path = array_path(graph, &format!("nodes/props/{name}/missing"));
    if Array::open(store.clone(), &path).is_err() {
        return Ok(Vec::new());
    }
    Ok(
        read_u8(store, graph, &format!("nodes/props/{name}/missing"))?
            .into_iter()
            .map(|value| value != 0)
            .collect(),
    )
}

fn read_ids(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    path: &str,
) -> Result<Vec<u64>, Error> {
    let array = open(store, graph, path)?;
    let dtype = array.data_type().clone();
    if dtype == data_type::uint64() {
        read_values::<u64>(&array, path)
    } else if dtype == data_type::uint32() {
        let raw = read_values::<u32>(&array, path)?;
        Ok(raw.into_iter().map(u64::from).collect())
    } else if dtype == data_type::int64() {
        let raw = read_values::<i64>(&array, path)?;
        raw.into_iter()
            .map(|value| {
                u64::try_from(value).map_err(|_| Error::Format {
                    detail: format!("the id {value} is negative"),
                })
            })
            .collect()
    } else if dtype == data_type::int32() {
        let raw = read_values::<i32>(&array, path)?;
        raw.into_iter()
            .map(|value| {
                u64::try_from(value).map_err(|_| Error::Format {
                    detail: format!("the id {value} is negative"),
                })
            })
            .collect()
    } else {
        Err(Error::Format {
            detail: format!("the ids at {path} are not integers"),
        })
    }
}

fn read_u8(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    path: &str,
) -> Result<Vec<u8>, Error> {
    let array = open(store, graph, path)?;
    let dtype = array.data_type().clone();
    if dtype == data_type::uint8() || dtype == data_type::bool() {
        read_values::<u8>(&array, path)
    } else {
        Err(Error::Format {
            detail: format!("the flags at {path} are not bytes"),
        })
    }
}

fn read_numbers(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    path: &str,
) -> Result<Vec<f64>, Error> {
    let array = open(store, graph, path)?;
    let dtype = array.data_type().clone();
    if dtype == data_type::float32() {
        Ok(read_values::<f32>(&array, path)?
            .into_iter()
            .map(f64::from)
            .collect())
    } else if dtype == data_type::float64() {
        Ok(read_values::<f64>(&array, path)?)
    } else if dtype == data_type::uint8() {
        Ok(read_values::<u8>(&array, path)?
            .into_iter()
            .map(f64::from)
            .collect())
    } else if dtype == data_type::uint16() {
        Ok(read_values::<u16>(&array, path)?
            .into_iter()
            .map(f64::from)
            .collect())
    } else if dtype == data_type::uint32() {
        Ok(read_values::<u32>(&array, path)?
            .into_iter()
            .map(f64::from)
            .collect())
    } else if dtype == data_type::uint64() {
        Ok(read_values::<u64>(&array, path)?
            .into_iter()
            .map(|value| value as f64)
            .collect())
    } else if dtype == data_type::int32() {
        Ok(read_values::<i32>(&array, path)?
            .into_iter()
            .map(f64::from)
            .collect())
    } else if dtype == data_type::int64() {
        Ok(read_values::<i64>(&array, path)?
            .into_iter()
            .map(|value| value as f64)
            .collect())
    } else {
        Err(Error::Format {
            detail: format!("the property at {path} is not numeric"),
        })
    }
}

fn open(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    path: &str,
) -> Result<Array<FilesystemStore>, Error> {
    let path = array_path(graph, path);
    Array::open(store.clone(), &path).map_err(|error| Error::Read {
        detail: format!("{path}: {error}"),
    })
}

fn read_values<T>(array: &Array<FilesystemStore>, path: &str) -> Result<Vec<T>, Error>
where
    T: zarrs::array::ElementOwned,
{
    let shape = array.shape().to_vec();
    let subset = ArraySubset::new_with_shape(shape);
    array
        .retrieve_array_subset::<Vec<T>>(&subset)
        .map_err(|error| Error::Read {
            detail: format!("{path}: {error}"),
        })
}

fn graph(store: &Arc<FilesystemStore>) -> Result<Group<FilesystemStore>, Error> {
    let root = Group::open(store.clone(), "/").map_err(|error| Error::Read {
        detail: error.to_string(),
    })?;
    if root.attributes().contains_key("geff") {
        return Ok(root);
    }
    let mut groups = root
        .child_groups()
        .map_err(|error| Error::Read {
            detail: error.to_string(),
        })?
        .into_iter()
        .filter(|group| group.attributes().contains_key("geff"))
        .collect::<Vec<_>>();
    groups.sort_unstable_by_key(|group| group.path().as_str().to_string());
    match groups.len() {
        0 => Err(Error::Format {
            detail: "the zarr store holds no geff graph".to_string(),
        }),
        1 => Ok(groups.remove(0)),
        _ => Err(Error::Format {
            detail: format!(
                "the zarr store holds {} geff graphs: {}",
                groups.len(),
                groups
                    .iter()
                    .map(|group| group.path().as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

fn array_path(graph: &Group<FilesystemStore>, path: &str) -> String {
    format!("{}/{path}", graph.path().as_str().trim_end_matches('/'))
}

fn kinds(lineage: &Lineage) -> Vec<u8> {
    let mut outgoing = HashMap::<u64, u32>::new();
    let mut incoming = HashMap::<u64, u32>::new();
    for edge in lineage.edges() {
        *outgoing.entry(edge.source()).or_default() += 1;
        *incoming.entry(edge.target()).or_default() += 1;
    }
    let times = lineage
        .detections()
        .iter()
        .map(|detection| (detection.id(), detection.time()))
        .collect::<BTreeMap<_, _>>();
    lineage
        .edges()
        .iter()
        .map(|edge| {
            if outgoing.get(&edge.source()) == Some(&2) {
                DIVISION
            } else if incoming.get(&edge.target()) == Some(&2) {
                MERGE
            } else if times[&edge.target()] == times[&edge.source()] + 1 {
                CONTINUE
            } else {
                GAP
            }
        })
        .collect()
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
