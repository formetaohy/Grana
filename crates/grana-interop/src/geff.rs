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

pub use crate::graph::{Kind, Mesh, Objects, Related, Shape};

pub const VERSION: &str = "1.0.0";

const CONTINUE: u8 = 0;
const GAP: u8 = 1;
const DIVISION: u8 = 2;
const MERGE: u8 = 3;

const POLYGON: &str = "polygon";
const MESH_VERTEX: &str = "grana_mesh_vertex";
const MESH_NORMAL: &str = "grana_mesh_normal";
const MESH_INDEX: &str = "grana_mesh_index";
const INTENSITY: &str = "grana_intensity";
const POLYGON_LIMIT: usize = 96;

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
    store(path, lineage, axes, None)
}

pub fn write_objects(
    path: &Path,
    lineage: &Lineage,
    axes: &Axes,
    objects: &Objects,
) -> Result<(), Error> {
    if objects.shapes().len() != lineage.len() || objects.labels().len() != lineage.len() {
        return Err(Error::Graph {
            detail: "the graph shapes do not cover every node".to_string(),
        });
    }
    if objects
        .intensities()
        .is_some_and(|intensities| intensities.len() != lineage.len())
    {
        return Err(Error::Graph {
            detail: "the graph intensities do not cover every node".to_string(),
        });
    }
    store(path, lineage, axes, Some(objects))
}

fn store(
    path: &Path,
    lineage: &Lineage,
    axes: &Axes,
    objects: Option<&Objects>,
) -> Result<(), Error> {
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
    let mut attributes = json!({
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
            "track_node_props": {
                "tracklet": "tracklet_id",
                "lineage": "lineage_id",
            },
            "node_props_metadata": {
                "t": property("t", "uint32", Some("frame")),
                "z": property("z", "float32", Some("micrometer")),
                "y": property("y", "float32", Some("micrometer")),
                "x": property("x", "float32", Some("micrometer")),
                "radius": property("radius", "float32", Some("micrometer")),
                "quality": property("quality", "float32", None),
                "tracklet_id": property("tracklet_id", "uint64", None),
                "lineage_id": property("lineage_id", "uint64", None),
            },
            "edge_props_metadata": {
                "grana_kind": property("grana_kind", "uint8", None),
            },
            "extra": {
                "grana": {
                    "frame_interval": axes.interval,
                    "voxel": axes.spacing,
                    "edge_kind": "grana_kind",
                },
            },
        },
    });
    declare_objects(&mut attributes, objects);
    group(
        &store,
        "/",
        attributes.as_object().cloned().unwrap_or_default(),
    )?;
    group(&store, "/nodes", Map::new())?;
    group(&store, "/nodes/props", Map::new())?;
    for name in [
        "t",
        "x",
        "y",
        "z",
        "radius",
        "quality",
        "tracklet_id",
        "lineage_id",
    ] {
        group(&store, &format!("/nodes/props/{name}"), Map::new())?;
    }
    if let Some(objects) = objects {
        for name in object_properties(objects.kind(), objects.channels() > 0) {
            group(&store, &format!("/nodes/props/{name}"), Map::new())?;
        }
    }
    group(&store, "/edges", Map::new())?;
    group(&store, "/edges/props", Map::new())?;
    group(&store, "/edges/props/grana_kind", Map::new())?;
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
    let tracklets = lineage.tracklets();
    array(
        &store,
        "/nodes/props/tracklet_id/values",
        vec![count as u64],
        data_type::uint64(),
        0u64,
        &tracklets,
    )?;
    let lineages = lineage.lineages();
    array(
        &store,
        "/nodes/props/lineage_id/values",
        vec![count as u64],
        data_type::uint64(),
        0u64,
        &lineages,
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
        "/edges/props/grana_kind/values",
        vec![rows],
        data_type::uint8(),
        0u8,
        &kind,
    )?;
    if let Some(objects) = objects {
        write_shapes(&store, lineage, objects)?;
    }
    Ok(())
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
    let mut seen = HashMap::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        if seen.insert(*id, slot).is_some() {
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
    let recorded = read_kinds(&store, &graph, metadata, &edges)?;
    let tracklets = read_identity(&store, &graph, names.tracklets.as_deref(), ids.len())?;
    let lineages = read_identity(&store, &graph, names.lineages.as_deref(), ids.len())?;
    let keys = edges
        .iter()
        .map(|edge| (edge.source(), edge.target()))
        .collect::<Vec<_>>();
    let slots = slots(&ids);
    let lineage = Lineage::new(detections, edges)?;
    if let Some(recorded) = recorded {
        validate_kinds(&lineage, &keys, &recorded)?;
    }
    validate_identity(&lineage, &slots, tracklets.as_deref(), Identity::Tracklet)?;
    validate_identity(&lineage, &slots, lineages.as_deref(), Identity::Lineage)?;
    Ok(lineage)
}

pub fn kind(path: &Path) -> Result<Kind, Error> {
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
    graph_kind(metadata)
}

pub fn read_objects(path: &Path, default_radius: f32) -> Result<(Lineage, Objects), Error> {
    let lineage = read(path, default_radius)?;
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
    let kind = graph_kind(metadata)?;
    let related = read_related(metadata)?;
    let ids = read_ids(&store, &graph, "nodes/ids")?;
    if ids.len() != lineage.len() {
        return Err(Error::Format {
            detail: "the graph nodes do not cover its detections".to_string(),
        });
    }
    let labels = read_labels(&store, &graph, &ids, metadata)?;
    let intensities = read_intensities(&store, &graph, &ids, metadata)?;
    let shapes = read_shapes(&store, &graph, &ids, kind)?;
    let mut pending = HashMap::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        pending.insert(*id, (shapes[slot].clone(), labels[slot]));
    }
    let mut ordered = Vec::with_capacity(lineage.len());
    let mut ordered_labels = Vec::with_capacity(lineage.len());
    for detection in lineage.detections() {
        let (shape, label) = pending
            .remove(&detection.id())
            .ok_or_else(|| Error::Format {
                detail: format!("the node {} carries no shape", detection.id()),
            })?;
        ordered.push(shape);
        ordered_labels.push(label);
    }
    let objects = Objects::new(kind, ordered, ordered_labels, intensities, related);
    Ok((lineage, objects))
}

fn graph_kind(metadata: &Map<String, Value>) -> Result<Kind, Error> {
    let Some(name) = metadata
        .get("extra")
        .and_then(|extra| extra.get("grana"))
        .and_then(|grana| grana.get("kind"))
        .and_then(Value::as_str)
    else {
        return Ok(Kind::Points);
    };
    Kind::parse(name).ok_or_else(|| Error::Format {
        detail: format!("the graph kind {name} is unknown"),
    })
}

fn read_related(metadata: &Map<String, Value>) -> Result<Vec<Related>, Error> {
    let Some(list) = metadata.get("related_objects").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut related = Vec::with_capacity(list.len());
    for value in list {
        let object = value.as_object().ok_or_else(|| Error::Format {
            detail: "a related object is not an object".to_string(),
        })?;
        let object_type = object
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| !kind.trim().is_empty())
            .ok_or_else(|| Error::Format {
                detail: "a related object names no type".to_string(),
            })?;
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| Error::Format {
                detail: format!("the related {object_type} names no path"),
            })?;
        let node_prop = match object.get("node_prop") {
            Some(Value::String(name)) if !name.trim().is_empty() => Some(name.as_str()),
            Some(_) => {
                return Err(Error::Format {
                    detail: format!("the related {object_type} names an invalid node property"),
                });
            }
            None => None,
        };
        related.push(Related::new(object_type, path, node_prop));
    }
    Ok(related)
}

fn read_labels(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    ids: &[u64],
    metadata: &Map<String, Value>,
) -> Result<Vec<u64>, Error> {
    if !declared(metadata, "seg_id") {
        return Ok((1..=ids.len() as u64).collect());
    }
    let values = read_ids(store, graph, "nodes/props/seg_id/values")?;
    if values.len() != ids.len() {
        return Err(Error::Format {
            detail: "the segmentation labels do not cover every node".to_string(),
        });
    }
    if values.contains(&0) {
        return Err(Error::Format {
            detail: "a segmentation label is zero".to_string(),
        });
    }
    Ok(values)
}

fn read_intensities(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    ids: &[u64],
    metadata: &Map<String, Value>,
) -> Result<Option<Vec<Vec<grana_object::SampleStatistics>>>, Error> {
    if !declared(metadata, INTENSITY) {
        return Ok(None);
    }
    let path = format!("nodes/props/{INTENSITY}/values");
    let array = open(store, graph, &path)?;
    let shape = array.shape().to_vec();
    if shape.len() != 3 || shape[0] as usize != ids.len() || shape[2] != 5 || shape[1] == 0 {
        return Err(Error::Format {
            detail: "the graph intensities are not shaped as nodes by channels by five statistics"
                .to_string(),
        });
    }
    let channels = shape[1] as usize;
    let values = read_values::<f32>(&array, &path)?;
    if values.len() != ids.len() * channels * 5 {
        return Err(Error::Format {
            detail: "the graph intensities do not cover every node".to_string(),
        });
    }
    let mut intensities = Vec::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        let mut samples = Vec::with_capacity(channels);
        for channel in 0..channels {
            let base = (slot * channels + channel) * 5;
            let values = &values[base..base + 5];
            if !values.iter().all(|value| value.is_finite())
                || values[1] > values[0]
                || values[0] > values[2]
                || values[3] < 0.0
                || values[4] < 0.0
            {
                return Err(Error::Format {
                    detail: format!("the node {id} carries degenerate intensities"),
                });
            }
            samples.push(grana_object::SampleStatistics::new(
                values[0],
                values[1],
                values[2],
                values[3],
                f64::from(values[4]),
            ));
        }
        intensities.push(samples);
    }
    Ok(Some(intensities))
}

fn read_shapes(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    ids: &[u64],
    kind: Kind,
) -> Result<Vec<Shape>, Error> {
    if kind == Kind::Points {
        return Ok(vec![Shape::Point; ids.len()]);
    }
    let vertices = read_variable(store, graph, MESH_VERTEX, 2)?;
    let normals = read_variable(store, graph, MESH_NORMAL, 2)?;
    let indices = read_variable(store, graph, MESH_INDEX, 1)?;
    let vertex_values = read_numbers(store, graph, &format!("nodes/props/{MESH_VERTEX}/data"))?;
    let normal_values = read_numbers(store, graph, &format!("nodes/props/{MESH_NORMAL}/data"))?;
    let index_values = read_ids(store, graph, &format!("nodes/props/{MESH_INDEX}/data"))?;
    let statistics = match kind {
        Kind::Surfaces => Statistics::surfaces(store, graph, ids.len())?,
        Kind::Filaments => Statistics::filaments(store, graph, ids.len())?,
        Kind::Points => unreachable!("a point graph carries no mesh"),
    };
    let mut shapes = Vec::with_capacity(ids.len());
    for (slot, id) in ids.iter().enumerate() {
        let positions = mesh_positions(&vertices, &vertex_values, slot, *id)?;
        let normals = mesh_positions(&normals, &normal_values, slot, *id)?;
        let triangles = mesh_indices(&indices, &index_values, slot, *id)?;
        let mesh = Mesh::new(positions, normals, triangles);
        shapes.push(match kind {
            Kind::Surfaces => Shape::Surface {
                mesh,
                voxels: statistics.voxels[slot],
                volume: statistics.volume[slot],
                area: statistics.area[slot],
            },
            Kind::Filaments => Shape::Filament {
                mesh,
                length: statistics.length[slot],
                diameter: statistics.diameter[slot],
                volume: statistics.volume[slot],
                branches: statistics.branches[slot],
                terminals: statistics.terminals[slot],
                junctions: statistics.junctions[slot],
                nodes: statistics.nodes[slot],
            },
            Kind::Points => unreachable!("a point graph carries no mesh"),
        });
    }
    Ok(shapes)
}

struct Statistics {
    voxels: Vec<u64>,
    volume: Vec<f64>,
    area: Vec<f64>,
    length: Vec<f64>,
    diameter: Vec<f64>,
    branches: Vec<u32>,
    terminals: Vec<u32>,
    junctions: Vec<u32>,
    nodes: Vec<u32>,
}

impl Statistics {
    fn surfaces(
        store: &Arc<FilesystemStore>,
        graph: &Group<FilesystemStore>,
        count: usize,
    ) -> Result<Self, Error> {
        let voxels = read_node_ids(store, graph, "grana_voxels", count)?;
        let volume = read_node_numbers(store, graph, "grana_volume", count)?;
        let area = read_node_numbers(store, graph, "grana_area", count)?;
        if voxels.contains(&0)
            || volume
                .iter()
                .any(|value| !(value.is_finite() && *value > 0.0))
            || area
                .iter()
                .any(|value| !(value.is_finite() && *value > 0.0))
        {
            return Err(Error::Format {
                detail: "a surface carries a degenerate statistic".to_string(),
            });
        }
        Ok(Self {
            voxels,
            volume,
            area,
            length: Vec::new(),
            diameter: Vec::new(),
            branches: Vec::new(),
            terminals: Vec::new(),
            junctions: Vec::new(),
            nodes: Vec::new(),
        })
    }

    fn filaments(
        store: &Arc<FilesystemStore>,
        graph: &Group<FilesystemStore>,
        count: usize,
    ) -> Result<Self, Error> {
        let length = read_node_numbers(store, graph, "grana_length", count)?;
        let diameter = read_node_numbers(store, graph, "grana_diameter", count)?;
        let volume = read_node_numbers(store, graph, "grana_volume", count)?;
        let branches = read_node_ids(store, graph, "grana_branches", count)?;
        let terminals = read_node_ids(store, graph, "grana_terminals", count)?;
        let junctions = read_node_ids(store, graph, "grana_junctions", count)?;
        let nodes = read_node_ids(store, graph, "grana_nodes", count)?;
        let finite = |values: &[f64]| values.iter().all(|value| value.is_finite() && *value > 0.0);
        if !finite(&length) || !finite(&diameter) || !finite(&volume) {
            return Err(Error::Format {
                detail: "a filament carries a degenerate statistic".to_string(),
            });
        }
        let mut topology = Vec::with_capacity(count);
        for slot in 0..count {
            let branches = u32::try_from(branches[slot]).map_err(|_| Error::Format {
                detail: "a filament counts more branches than a graph holds".to_string(),
            })?;
            let terminals = u32::try_from(terminals[slot]).map_err(|_| Error::Format {
                detail: "a filament counts more terminals than a graph holds".to_string(),
            })?;
            let junctions = u32::try_from(junctions[slot]).map_err(|_| Error::Format {
                detail: "a filament counts more junctions than a graph holds".to_string(),
            })?;
            let nodes = u32::try_from(nodes[slot]).map_err(|_| Error::Format {
                detail: "a filament counts more nodes than a graph holds".to_string(),
            })?;
            if branches == 0 || nodes == 0 || terminals > 2 * branches || junctions >= branches {
                return Err(Error::Format {
                    detail: "a filament carries an inconsistent topology".to_string(),
                });
            }
            topology.push((branches, terminals, junctions, nodes));
        }
        Ok(Self {
            voxels: Vec::new(),
            volume,
            area: Vec::new(),
            length,
            diameter,
            branches: topology.iter().map(|value| value.0).collect(),
            terminals: topology.iter().map(|value| value.1).collect(),
            junctions: topology.iter().map(|value| value.2).collect(),
            nodes: topology.iter().map(|value| value.3).collect(),
        })
    }
}

fn read_node_ids(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: &str,
    count: usize,
) -> Result<Vec<u64>, Error> {
    let values = read_ids(store, graph, &format!("nodes/props/{name}/values"))?;
    if values.len() != count {
        return Err(Error::Format {
            detail: format!("the {name} property does not cover every node"),
        });
    }
    Ok(values)
}

fn read_node_numbers(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: &str,
    count: usize,
) -> Result<Vec<f64>, Error> {
    let values = read_numbers(store, graph, &format!("nodes/props/{name}/values"))?;
    if values.len() != count {
        return Err(Error::Format {
            detail: format!("the {name} property does not cover every node"),
        });
    }
    Ok(values)
}

struct Variable {
    offsets: Vec<u64>,
    dims: Vec<Vec<u64>>,
}

fn read_variable(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: &str,
    dims: usize,
) -> Result<Variable, Error> {
    let path = format!("nodes/props/{name}/values");
    let array = open(store, graph, &path)?;
    let shape = array.shape().to_vec();
    if shape.len() != 2 || shape[1] as usize != dims + 1 {
        return Err(Error::Format {
            detail: format!("the {name} rows do not hold {dims} dimensions"),
        });
    }
    let width = shape[1] as usize;
    let values = read_ids(store, graph, &path)?;
    if values.len() != shape[0] as usize * width {
        return Err(Error::Format {
            detail: format!("the {name} rows do not cover every node"),
        });
    }
    let mut offsets = Vec::with_capacity(shape[0] as usize);
    let mut rows = Vec::with_capacity(shape[0] as usize);
    for row in values.chunks(width) {
        offsets.push(row[0]);
        rows.push(row[1..].to_vec());
    }
    Ok(Variable {
        offsets,
        dims: rows,
    })
}

fn mesh_positions(
    rows: &Variable,
    values: &[f64],
    slot: usize,
    id: u64,
) -> Result<Vec<[f32; 3]>, Error> {
    let dims = &rows.dims[slot];
    if dims.len() != 2 || dims[1] != 3 || dims[0] == 0 {
        return Err(Error::Format {
            detail: format!("the node {id} carries no vertex mesh"),
        });
    }
    let count = dims[0] as usize;
    let start = usize::try_from(rows.offsets[slot]).map_err(|_| Error::Format {
        detail: format!("the node {id} names an unaddressable vertex offset"),
    })?;
    let end = start.checked_add(count * 3).ok_or_else(|| Error::Format {
        detail: format!("the node {id} names an unaddressable vertex range"),
    })?;
    let values = values.get(start..end).ok_or_else(|| Error::Format {
        detail: format!("the node {id} names vertices outside the graph"),
    })?;
    let mut positions = Vec::with_capacity(count);
    for point in values.chunks(3) {
        let position = [point[0] as f32, point[1] as f32, point[2] as f32];
        if position.iter().any(|value| !value.is_finite()) {
            return Err(Error::Format {
                detail: format!("the node {id} carries a non finite vertex"),
            });
        }
        positions.push(position);
    }
    Ok(positions)
}

fn mesh_indices(rows: &Variable, values: &[u64], slot: usize, id: u64) -> Result<Vec<u32>, Error> {
    let dims = &rows.dims[slot];
    if dims.len() != 1 || dims[0] == 0 || !dims[0].is_multiple_of(3) {
        return Err(Error::Format {
            detail: format!("the node {id} carries no triangle mesh"),
        });
    }
    let count = dims[0] as usize;
    let start = usize::try_from(rows.offsets[slot]).map_err(|_| Error::Format {
        detail: format!("the node {id} names an unaddressable index offset"),
    })?;
    let end = start.checked_add(count).ok_or_else(|| Error::Format {
        detail: format!("the node {id} names an unaddressable index range"),
    })?;
    let values = values.get(start..end).ok_or_else(|| Error::Format {
        detail: format!("the node {id} names indices outside the graph"),
    })?;
    values
        .iter()
        .map(|value| {
            u32::try_from(*value).map_err(|_| Error::Format {
                detail: format!("the node {id} names an unaddressable vertex"),
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Identity {
    Tracklet,
    Lineage,
}

impl Identity {
    fn name(self) -> &'static str {
        match self {
            Self::Tracklet => "tracklet",
            Self::Lineage => "lineage",
        }
    }

    fn values(self, lineage: &Lineage) -> Vec<u64> {
        match self {
            Self::Tracklet => lineage.tracklets(),
            Self::Lineage => lineage.lineages(),
        }
    }
}

struct Names {
    time: String,
    space: [Option<String>; 3],
    sphere: String,
    tracklets: Option<String>,
    lineages: Option<String>,
}

fn names(metadata: &Map<String, Value>) -> Result<Names, Error> {
    let sphere = metadata
        .get("sphere")
        .and_then(Value::as_str)
        .unwrap_or("radius")
        .to_string();
    let mut tracklets = None;
    let mut lineages = None;
    if let Some(value) = metadata.get("track_node_props") {
        let props = value.as_object().ok_or_else(|| Error::Format {
            detail: "the track node properties are not an object".to_string(),
        })?;
        for (key, value) in props {
            let name = value.as_str().ok_or_else(|| Error::Format {
                detail: format!("the {key} ids are not a property name"),
            })?;
            if name.is_empty() {
                return Err(Error::Format {
                    detail: format!("the {key} ids name no property"),
                });
            }
            match key.as_str() {
                "tracklet" => tracklets = Some(name.to_string()),
                "lineage" => lineages = Some(name.to_string()),
                other => {
                    return Err(Error::Format {
                        detail: format!("the track node properties name the unknown {other}"),
                    });
                }
            }
        }
        for (key, name) in [("tracklet", &tracklets), ("lineage", &lineages)] {
            if let Some(name) = name
                && !declared(metadata, name)
            {
                return Err(Error::Format {
                    detail: format!("the {key} ids name the undeclared property {name}"),
                });
            }
        }
    }
    let Some(list) = metadata.get("axes").and_then(Value::as_array) else {
        return Ok(Names {
            time: "t".to_string(),
            space: [
                Some("x".to_string()),
                Some("y".to_string()),
                Some("z".to_string()),
            ],
            sphere,
            tracklets,
            lineages,
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
        tracklets,
        lineages,
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

fn declared_edge(metadata: &Map<String, Value>, name: &str) -> bool {
    metadata
        .get("edge_props_metadata")
        .and_then(Value::as_object)
        .is_some_and(|props| props.contains_key(name))
}

fn slots(ids: &[u64]) -> HashMap<u64, usize> {
    ids.iter()
        .enumerate()
        .map(|(slot, id)| (*id, slot))
        .collect()
}

fn read_kinds(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    metadata: &Map<String, Value>,
    edges: &[Edge],
) -> Result<Option<Vec<u8>>, Error> {
    let Some(name) = edge_kind(metadata) else {
        return Ok(None);
    };
    if !declared_edge(metadata, &name) {
        return Err(Error::Format {
            detail: format!("the edge kinds name the undeclared property {name}"),
        });
    }
    if edges.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let values = read_u8(store, graph, &format!("edges/props/{name}/values"))?;
    if values.len() != edges.len() {
        return Err(Error::Format {
            detail: "the edge kinds do not cover every edge".to_string(),
        });
    }
    if let Some(value) = values.iter().find(|value| **value > MERGE) {
        return Err(Error::Format {
            detail: format!("the edge kind {value} is unknown"),
        });
    }
    Ok(Some(values))
}

fn edge_kind(metadata: &Map<String, Value>) -> Option<String> {
    metadata
        .get("extra")
        .and_then(|extra| extra.get("grana"))
        .and_then(|grana| grana.get("edge_kind"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn read_identity(
    store: &Arc<FilesystemStore>,
    graph: &Group<FilesystemStore>,
    name: Option<&str>,
    count: usize,
) -> Result<Option<Vec<u64>>, Error> {
    let Some(name) = name else {
        return Ok(None);
    };
    let values = read_ids(store, graph, &format!("nodes/props/{name}/values"))?;
    if values.len() != count {
        return Err(Error::Format {
            detail: format!("the {name} property does not cover every node"),
        });
    }
    Ok(Some(values))
}

fn validate_kinds(lineage: &Lineage, keys: &[(u64, u64)], values: &[u8]) -> Result<(), Error> {
    let recorded = keys
        .iter()
        .copied()
        .zip(values.iter().copied())
        .collect::<HashMap<_, _>>();
    for (edge, expected) in lineage.edges().iter().zip(kinds(lineage)) {
        let key = (edge.source(), edge.target());
        let Some(value) = recorded.get(&key) else {
            return Err(Error::Format {
                detail: format!(
                    "the edge kinds skip the edge {} to {}",
                    edge.source(),
                    edge.target()
                ),
            });
        };
        if *value != expected {
            return Err(Error::Format {
                detail: format!(
                    "the edge {} to {} carries the kind {value} where the graph holds {expected}",
                    edge.source(),
                    edge.target()
                ),
            });
        }
    }
    Ok(())
}

fn validate_identity(
    lineage: &Lineage,
    slots: &HashMap<u64, usize>,
    values: Option<&[u64]>,
    kind: Identity,
) -> Result<(), Error> {
    let Some(values) = values else {
        return Ok(());
    };
    let expected = kind.values(lineage);
    let mut by_expected = HashMap::<u64, u64>::new();
    let mut by_value = HashMap::<u64, u64>::new();
    for (index, detection) in lineage.detections().iter().enumerate() {
        let value = values[slots[&detection.id()]];
        if by_expected
            .insert(expected[index], value)
            .is_some_and(|previous| previous != value)
            || by_value
                .insert(value, expected[index])
                .is_some_and(|previous| previous != expected[index])
        {
            return Err(Error::Format {
                detail: format!("the {} ids do not follow the graph", kind.name()),
            });
        }
    }
    Ok(())
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

fn variable(name: &str, dtype: &str) -> Value {
    let mut property = Map::new();
    property.insert("identifier".to_string(), json!(name));
    property.insert("dtype".to_string(), json!(dtype));
    property.insert("varlength".to_string(), json!(true));
    Value::Object(property)
}

fn declare_objects(attributes: &mut Value, objects: Option<&Objects>) {
    let Some(objects) = objects else {
        return;
    };
    let Some(geff) = attributes.get_mut("geff").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(properties) = geff
        .get_mut("node_props_metadata")
        .and_then(Value::as_object_mut)
    {
        properties.insert("seg_id".to_string(), property("seg_id", "uint64", None));
        properties.insert(POLYGON.to_string(), variable(POLYGON, "float32"));
        if objects.kind() != Kind::Points {
            properties.insert(MESH_VERTEX.to_string(), variable(MESH_VERTEX, "float32"));
            properties.insert(MESH_NORMAL.to_string(), variable(MESH_NORMAL, "float32"));
            properties.insert(MESH_INDEX.to_string(), variable(MESH_INDEX, "uint32"));
            match objects.kind() {
                Kind::Surfaces => {
                    properties.insert(
                        "grana_voxels".to_string(),
                        property("grana_voxels", "uint64", None),
                    );
                    properties.insert(
                        "grana_volume".to_string(),
                        property("grana_volume", "float32", None),
                    );
                    properties.insert(
                        "grana_area".to_string(),
                        property("grana_area", "float32", None),
                    );
                }
                Kind::Filaments => {
                    for name in ["grana_length", "grana_diameter", "grana_volume"] {
                        properties.insert(name.to_string(), property(name, "float32", None));
                    }
                    for name in [
                        "grana_branches",
                        "grana_terminals",
                        "grana_junctions",
                        "grana_nodes",
                    ] {
                        properties.insert(name.to_string(), property(name, "uint32", None));
                    }
                }
                Kind::Points => {}
            }
        }
        if objects.channels() > 0 {
            properties.insert(INTENSITY.to_string(), property(INTENSITY, "float32", None));
        }
    }
    if let Some(grana) = geff
        .get_mut("extra")
        .and_then(|extra| extra.get_mut("grana"))
        .and_then(Value::as_object_mut)
    {
        grana.insert("kind".to_string(), json!(objects.kind().name()));
    }
    geff.insert("polygon".to_string(), json!(POLYGON));
    if !objects.related().is_empty() {
        let related = objects
            .related()
            .iter()
            .map(|related| {
                let mut value = Map::new();
                value.insert("type".to_string(), json!(related.object_type()));
                value.insert("path".to_string(), json!(related.path()));
                if let Some(property) = related.node_prop() {
                    value.insert("node_prop".to_string(), json!(property));
                }
                Value::Object(value)
            })
            .collect::<Vec<_>>();
        geff.insert("related_objects".to_string(), Value::Array(related));
    }
}

fn object_properties(kind: Kind, intensities: bool) -> Vec<&'static str> {
    let mut names = vec!["seg_id", POLYGON];
    match kind {
        Kind::Points => {}
        Kind::Surfaces => names.extend([
            MESH_VERTEX,
            MESH_NORMAL,
            MESH_INDEX,
            "grana_voxels",
            "grana_volume",
            "grana_area",
        ]),
        Kind::Filaments => names.extend([
            MESH_VERTEX,
            MESH_NORMAL,
            MESH_INDEX,
            "grana_length",
            "grana_diameter",
            "grana_volume",
            "grana_branches",
            "grana_terminals",
            "grana_junctions",
            "grana_nodes",
        ]),
    }
    if intensities {
        names.push(INTENSITY);
    }
    names
}

fn write_shapes(
    store: &Arc<FilesystemStore>,
    lineage: &Lineage,
    objects: &Objects,
) -> Result<(), Error> {
    let count = lineage.len() as u64;
    array(
        store,
        "/nodes/props/seg_id/values",
        vec![count],
        data_type::uint64(),
        0u64,
        objects.labels(),
    )?;
    let mut polygons = Vec::with_capacity(lineage.len() * 3);
    let mut points = Vec::new();
    let mut vertex_rows = Vec::new();
    let mut vertex_values = Vec::new();
    let mut normal_rows = Vec::new();
    let mut normal_values = Vec::new();
    let mut index_rows = Vec::new();
    let mut index_values = Vec::new();
    for (detection, shape) in lineage.detections().iter().zip(objects.shapes()) {
        let polygon = footprint(detection, shape);
        polygons.extend([points.len() as u64, polygon.len() as u64, 3]);
        for point in &polygon {
            points.extend_from_slice(point);
        }
        if let Some(mesh) = shape.mesh() {
            vertex_rows.extend([vertex_values.len() as u64, mesh.positions().len() as u64, 3]);
            normal_rows.extend([normal_values.len() as u64, mesh.normals().len() as u64, 3]);
            index_rows.extend([index_values.len() as u64, mesh.indices().len() as u64]);
            for position in mesh.positions() {
                vertex_values.extend_from_slice(position);
            }
            for normal in mesh.normals() {
                normal_values.extend_from_slice(normal);
            }
            index_values.extend_from_slice(mesh.indices());
        }
    }
    array(
        store,
        "/nodes/props/polygon/values",
        vec![count, 3],
        data_type::uint64(),
        0u64,
        &polygons,
    )?;
    array(
        store,
        "/nodes/props/polygon/data",
        vec![points.len() as u64],
        data_type::float32(),
        0.0f32,
        &points,
    )?;
    if objects.kind() != Kind::Points {
        array(
            store,
            &format!("/nodes/props/{MESH_VERTEX}/values"),
            vec![count, 3],
            data_type::uint64(),
            0u64,
            &vertex_rows,
        )?;
        array(
            store,
            &format!("/nodes/props/{MESH_VERTEX}/data"),
            vec![vertex_values.len() as u64],
            data_type::float32(),
            0.0f32,
            &vertex_values,
        )?;
        array(
            store,
            &format!("/nodes/props/{MESH_NORMAL}/values"),
            vec![count, 3],
            data_type::uint64(),
            0u64,
            &normal_rows,
        )?;
        array(
            store,
            &format!("/nodes/props/{MESH_NORMAL}/data"),
            vec![normal_values.len() as u64],
            data_type::float32(),
            0.0f32,
            &normal_values,
        )?;
        array(
            store,
            &format!("/nodes/props/{MESH_INDEX}/values"),
            vec![count, 2],
            data_type::uint64(),
            0u64,
            &index_rows,
        )?;
        array(
            store,
            &format!("/nodes/props/{MESH_INDEX}/data"),
            vec![index_values.len() as u64],
            data_type::uint32(),
            0u32,
            &index_values,
        )?;
        write_statistics(store, objects)?;
    }
    if let Some(intensities) = objects.intensities() {
        let channels = objects.channels();
        let mut values = Vec::with_capacity(intensities.len() * channels * 5);
        for samples in intensities {
            for sample in samples {
                values.extend([
                    sample.mean(),
                    sample.minimum(),
                    sample.maximum(),
                    sample.deviation(),
                    sample.sum() as f32,
                ]);
            }
        }
        array(
            store,
            &format!("/nodes/props/{INTENSITY}/values"),
            vec![count, channels as u64, 5],
            data_type::float32(),
            0.0f32,
            &values,
        )?;
    }
    Ok(())
}

fn write_statistics(store: &Arc<FilesystemStore>, objects: &Objects) -> Result<(), Error> {
    let mut voxels = Vec::new();
    let mut volume = Vec::new();
    let mut area = Vec::new();
    let mut length = Vec::new();
    let mut diameter = Vec::new();
    let mut branches = Vec::new();
    let mut terminals = Vec::new();
    let mut junctions = Vec::new();
    let mut nodes = Vec::new();
    for shape in objects.shapes() {
        match shape {
            Shape::Surface {
                voxels: count,
                volume: measured,
                area: spanned,
                ..
            } => {
                voxels.push(*count);
                volume.push(*measured as f32);
                area.push(*spanned as f32);
            }
            Shape::Filament {
                length: measured,
                diameter: width,
                volume: occupied,
                branches: forks,
                terminals: ends,
                junctions: joins,
                nodes: points,
                ..
            } => {
                length.push(*measured as f32);
                diameter.push(*width as f32);
                volume.push(*occupied as f32);
                branches.push(*forks);
                terminals.push(*ends);
                junctions.push(*joins);
                nodes.push(*points);
            }
            Shape::Point => {
                return Err(Error::Graph {
                    detail: "a point graph carries no surface statistics".to_string(),
                });
            }
        }
    }
    let count = objects.shapes().len() as u64;
    match objects.kind() {
        Kind::Surfaces => {
            array(
                store,
                "/nodes/props/grana_voxels/values",
                vec![count],
                data_type::uint64(),
                0u64,
                &voxels,
            )?;
            array(
                store,
                "/nodes/props/grana_volume/values",
                vec![count],
                data_type::float32(),
                0.0f32,
                &volume,
            )?;
            array(
                store,
                "/nodes/props/grana_area/values",
                vec![count],
                data_type::float32(),
                0.0f32,
                &area,
            )?;
        }
        Kind::Filaments => {
            array(
                store,
                "/nodes/props/grana_length/values",
                vec![count],
                data_type::float32(),
                0.0f32,
                &length,
            )?;
            array(
                store,
                "/nodes/props/grana_diameter/values",
                vec![count],
                data_type::float32(),
                0.0f32,
                &diameter,
            )?;
            array(
                store,
                "/nodes/props/grana_volume/values",
                vec![count],
                data_type::float32(),
                0.0f32,
                &volume,
            )?;
            array(
                store,
                "/nodes/props/grana_branches/values",
                vec![count],
                data_type::uint32(),
                0u32,
                &branches,
            )?;
            array(
                store,
                "/nodes/props/grana_terminals/values",
                vec![count],
                data_type::uint32(),
                0u32,
                &terminals,
            )?;
            array(
                store,
                "/nodes/props/grana_junctions/values",
                vec![count],
                data_type::uint32(),
                0u32,
                &junctions,
            )?;
            array(
                store,
                "/nodes/props/grana_nodes/values",
                vec![count],
                data_type::uint32(),
                0u32,
                &nodes,
            )?;
        }
        Kind::Points => {}
    }
    Ok(())
}

fn footprint(detection: &Detection, shape: &Shape) -> Vec<[f32; 3]> {
    let origin = detection.position();
    let projected = match shape.mesh() {
        Some(mesh) => mesh
            .positions()
            .iter()
            .map(|position| [position[0] - origin[0], position[1] - origin[1]])
            .collect::<Vec<_>>(),
        None => {
            let radius = detection.radius();
            (0..12)
                .map(|step| {
                    let angle = std::f32::consts::TAU * step as f32 / 12.0;
                    [radius * angle.cos(), radius * angle.sin()]
                })
                .collect()
        }
    };
    let mut hull = convex_hull(projected);
    if hull.len() < 3 {
        hull = vec![
            [-detection.radius(), -detection.radius()],
            [detection.radius(), -detection.radius()],
            [detection.radius(), detection.radius()],
            [-detection.radius(), detection.radius()],
        ];
    }
    if hull.len() > POLYGON_LIMIT {
        let stride = hull.len().div_ceil(POLYGON_LIMIT);
        hull = hull.into_iter().step_by(stride).collect();
    }
    hull.into_iter()
        .map(|point| [point[0], point[1], 0.0])
        .collect()
}

fn convex_hull(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    points.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then(left[1].total_cmp(&right[1]))
    });
    points.dedup();
    if points.len() <= 2 {
        return points;
    }
    let cross = |origin: [f32; 2], first: [f32; 2], second: [f32; 2]| {
        (first[0] - origin[0]) * (second[1] - origin[1])
            - (first[1] - origin[1]) * (second[0] - origin[0])
    };
    let mut lower = Vec::new();
    for point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in points.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], *point) <= 0.0
        {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
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
