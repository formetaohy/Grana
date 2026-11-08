use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use zarrs::array::codec::ZstdCodec;
use zarrs::array::{Array, ArrayBuilder, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::{Group, GroupBuilder};

use crate::Image;
use crate::error::Error;
use crate::metadata::{Axis, AxisKind};

const CHUNK_X: u32 = 256;
const CHUNK_Y: u32 = 256;
const CHUNK_Z: u32 = 32;
const LABEL_LIMIT: usize = u16::MAX as usize;

pub type ChunkFill<'a> =
    dyn FnMut(usize, u32, [u32; 3], [u32; 3], &mut [u16]) -> Result<(), String> + 'a;

pub struct LabelSpec<'a> {
    pub name: &'a str,
    pub frames: &'a [u32],
    pub labels: usize,
    pub color: [f32; 3],
    pub opacity: f32,
    pub properties: &'a [(u32, String)],
    pub fill: &'a mut ChunkFill<'a>,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

pub struct Written {
    path: PathBuf,
    name: String,
    labels: usize,
    frames: usize,
    bytes: u64,
}

impl Written {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn labels(&self) -> usize {
        self.labels
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

pub fn write(path: &Path, spec: LabelSpec<'_>) -> Result<Written, Error> {
    write_group(path, "/", spec)
}

pub fn write_group(path: &Path, group: &str, spec: LabelSpec<'_>) -> Result<Written, Error> {
    assert!(
        !spec.name.is_empty() && !spec.name.contains('/') && !spec.name.contains('\\'),
        "a label image name is a path segment"
    );
    assert!(
        (1..=LABEL_LIMIT).contains(&spec.labels),
        "a label image holds one to {LABEL_LIMIT} labels"
    );
    assert!(
        spec.color.iter().all(|value| (0.0..=1.0).contains(value)),
        "a label color is a unit rgb triple"
    );
    assert!(
        (0.0..=1.0).contains(&spec.opacity),
        "a label color is a unit opacity"
    );
    let directory = path.join(group.trim_start_matches('/'));
    if !directory.join("zarr.json").is_file() {
        return Err(Error::Unsupported {
            path: directory.display().to_string(),
            detail: "label images are written into OME-Zarr 0.6 (zarr v3) datasets".to_string(),
        });
    }
    let labels_group = crate::hcs::join(group, "labels");
    let root = directory.join("labels").join(spec.name);
    let temporary_name = format!(".{}.{}.tmp", spec.name, std::process::id());
    let temporary_group = crate::hcs::join(&labels_group, &temporary_name);
    let temporary = directory.join("labels").join(&temporary_name);
    if temporary.exists() {
        std::fs::remove_dir_all(&temporary).map_err(|error| Error::Write {
            path: directory.display().to_string(),
            detail: error.to_string(),
        })?;
    }
    let fail = |detail: String| {
        let _ = std::fs::remove_dir_all(&temporary);
        Error::Write {
            path: directory.display().to_string(),
            detail,
        }
    };
    let image = Image::open_group(path, group)?;
    if image.minor() < 5 {
        return Err(fail(format!(
            "label images are written into OME-Zarr 0.5 or newer datasets, not {}",
            image.version()
        )));
    }
    if spec.frames.is_empty()
        || spec
            .frames
            .iter()
            .any(|frame| *frame >= image.pyramid().frame_count())
    {
        return Err(fail(format!(
            "the exported frames must lie inside the {} frames of the dataset",
            image.pyramid().frame_count()
        )));
    }
    if spec.frames.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(fail(
            "the exported frames are ordered and unique".to_string(),
        ));
    }
    let axes = image.axes();
    let kinds = axes
        .iter()
        .map(kind)
        .collect::<Result<Vec<Kind>, String>>()
        .map_err(&fail)?;
    let space = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == Kind::Space)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if space.len() < 2 {
        return Err(fail(
            "the dataset does not name two spatial axes to label".to_string(),
        ));
    }
    let x = axes.len() - 1;
    let y = axes.len() - 2;
    let z = (space.len() == 3).then_some(axes.len() - 3);
    let levels = image.pyramid().level_count();
    if image.datasets().len() != levels {
        return Err(fail(
            "the dataset does not describe the scales of its levels".to_string(),
        ));
    }
    let axes_json = axes.iter().map(Axis::json).collect::<Vec<_>>();
    let datasets = image
        .datasets()
        .iter()
        .enumerate()
        .map(|(level, dataset)| {
            if dataset.scale.len() != axes.len() || dataset.translation.len() != axes.len() {
                return Err(fail(
                    "the level transform does not describe every axis of the dataset".to_string(),
                ));
            }
            if dataset
                .scale
                .iter()
                .chain(&dataset.translation)
                .any(|value| !value.is_finite())
            {
                return Err(fail(
                    "the level transform holds a non-finite component".to_string(),
                ));
            }
            Ok(if image.modern() {
                crate::schema::dataset(&level.to_string(), &dataset.scale, &dataset.translation)
            } else {
                json!({
                    "path": level.to_string(),
                    "coordinateTransformations": [
                        { "type": "scale", "scale": dataset.scale },
                        { "type": "translation", "translation": dataset.translation },
                    ],
                })
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let multiscales = if image.modern() {
        crate::schema::multiscales(spec.name, &axes_json, datasets)
    } else {
        json!([{
            "name": spec.name,
            "axes": axes_json,
            "datasets": datasets,
        }])
    };
    let colors = (1..=spec.labels)
        .map(|label| {
            json!({
                "label-value": label,
                "rgba": [
                    (spec.color[0] * 255.0).round() as u8,
                    (spec.color[1] * 255.0).round() as u8,
                    (spec.color[2] * 255.0).round() as u8,
                    (spec.opacity * 255.0).round() as u8,
                ],
            })
        })
        .collect::<Vec<_>>();
    let properties = spec
        .properties
        .iter()
        .map(|(label, name)| json!({ "label-value": label, "name": name }))
        .collect::<Vec<_>>();
    let depth = group
        .trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .count();
    let source = "../".repeat(depth + 2);
    let image_label = json!({
        "colors": colors,
        "properties": properties,
        "source": { "image": source },
    });
    let label_attributes = json!({
        "ome": {
            "version": image.version(),
            "multiscales": multiscales,
            "image-label": image_label,
        },
    });
    let store = Arc::new(FilesystemStore::new(path).map_err(|error| Error::Open {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?);
    let mut group_builder = GroupBuilder::new();
    group_builder.attributes(
        label_attributes
            .as_object()
            .expect("label attributes are a json object")
            .clone(),
    );
    let group = group_builder
        .build(store.clone(), &temporary_group)
        .map_err(|error| fail(error.to_string()))?;
    group
        .store_metadata()
        .map_err(|error| fail(error.to_string()))?;
    let chunks = (0..levels)
        .map(|level| {
            let extent = image.pyramid().level(level).extent();
            let chunk = chunk_shape(extent, &kinds, x, y, z);
            spatial_chunks(extent, &chunk, x, y, z)
        })
        .sum::<usize>()
        * spec.frames.len();
    let mut stats = Stats {
        done: 0,
        total: chunks,
        bytes: 0,
    };
    for level in 0..levels {
        let extent = image.pyramid().level(level).extent();
        let chunk = chunk_shape(extent, &kinds, x, y, z);
        let shape = shape(extent, image.pyramid().frame_count(), &kinds);
        let names = axes
            .iter()
            .map(|axis| axis.name().to_string())
            .collect::<Vec<_>>();
        let array = ArrayBuilder::new(shape, chunk.clone(), data_type::uint16(), 0u16)
            .bytes_to_bytes_codecs(vec![Arc::new(ZstdCodec::new(3, false))])
            .dimension_names(Some(names))
            .build(
                store.clone(),
                &crate::hcs::join(&temporary_group, &level.to_string()),
            )
            .map_err(|error| fail(error.to_string()))?;
        array
            .store_metadata()
            .map_err(|error| fail(error.to_string()))?;
        write_level(
            &array,
            Slice {
                image: &image,
                level,
                frames: spec.frames,
            },
            Geometry {
                kinds: &kinds,
                x,
                y,
                z,
                chunk: &chunk,
            },
            &mut stats,
            &mut *spec.fill,
            &mut *spec.progress,
        )
        .map_err(&fail)?;
    }
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| fail(error.to_string()))?;
    }
    std::fs::rename(&temporary, &root).map_err(|error| fail(error.to_string()))?;
    if let Err(detail) = register_labels(
        store.clone(),
        &directory,
        &labels_group,
        spec.name,
        image.version(),
    ) {
        let _ = std::fs::remove_dir_all(&root);
        return Err(fail(detail));
    }
    Ok(Written {
        path: root,
        name: spec.name.to_string(),
        labels: spec.labels,
        frames: spec.frames.len(),
        bytes: stats.bytes,
    })
}

struct Slice<'a> {
    image: &'a Image,
    level: usize,
    frames: &'a [u32],
}

struct Geometry<'a> {
    kinds: &'a [Kind],
    x: usize,
    y: usize,
    z: Option<usize>,
    chunk: &'a [u64],
}

struct Stats {
    done: usize,
    total: usize,
    bytes: u64,
}

fn write_level<S>(
    array: &Array<S>,
    slice: Slice<'_>,
    geometry: Geometry<'_>,
    stats: &mut Stats,
    fill: &mut ChunkFill<'_>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), String>
where
    S: ?Sized + zarrs::storage::WritableStorageTraits + 'static,
{
    let Slice {
        image,
        level,
        frames,
    } = slice;
    let Geometry {
        kinds,
        x,
        y,
        z,
        chunk,
    } = geometry;
    let extent = image.pyramid().level(level).extent();
    let spatial = [extent.x(), extent.y(), extent.z()];
    let depth = z.map_or(1, |z| chunk[z]);
    let depth_extent = if z.is_some() { spatial[2] } else { 1 };
    let counts = [
        u64::from(spatial[0]).div_ceil(chunk[x]),
        u64::from(spatial[1]).div_ceil(chunk[y]),
        u64::from(depth_extent).div_ceil(depth),
    ];
    let mut indices = vec![0u64; kinds.len()];
    for &frame in frames {
        for cz in 0..counts[2] {
            for cy in 0..counts[1] {
                for cx in 0..counts[0] {
                    let origin = [
                        (cx * chunk[x]) as u32,
                        (cy * chunk[y]) as u32,
                        (cz * depth) as u32,
                    ];
                    let length = [
                        (chunk[x]).min(u64::from(spatial[0] - origin[0])) as u32,
                        (chunk[y]).min(u64::from(spatial[1] - origin[1])) as u32,
                        depth.min(u64::from(depth_extent - origin[2])) as u32,
                    ];
                    let voxels = (length[0] as usize) * (length[1] as usize) * (length[2] as usize);
                    let mut data = vec![0u16; voxels];
                    fill(level, frame, origin, length, &mut data)?;
                    for (index, kind) in kinds.iter().enumerate() {
                        indices[index] = match kind {
                            Kind::Time => u64::from(frame),
                            Kind::Channel => 0,
                            Kind::Space if index == x => cx,
                            Kind::Space if index == y => cy,
                            Kind::Space => cz,
                        };
                    }
                    array
                        .store_chunk(&indices, data)
                        .map_err(|error| error.to_string())?;
                    stats.bytes += (voxels * 2) as u64;
                    stats.done += 1;
                    progress(stats.done, stats.total);
                }
            }
        }
    }
    Ok(())
}

fn register_labels(
    store: Arc<FilesystemStore>,
    directory: &Path,
    labels_group: &str,
    name: &str,
    version: &str,
) -> Result<(), String> {
    let present = directory.join("labels").join("zarr.json").is_file();
    let mut attributes = if present {
        let group = Group::open(store.clone(), labels_group).map_err(|error| error.to_string())?;
        group.attributes().clone()
    } else {
        Map::new()
    };
    let ome = attributes
        .entry("ome".to_string())
        .or_insert_with(|| json!({ "version": version }));
    let object = ome
        .as_object_mut()
        .ok_or_else(|| "the labels group carries invalid ome metadata".to_string())?;
    let held = object
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or(version);
    if held != version {
        return Err(format!(
            "the dataset labels its images with OME-Zarr {held}, not {version}"
        ));
    }
    let labels = object
        .entry("labels".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| "the labels group carries an invalid labels list".to_string())?;
    if !labels.iter().any(|value| value.as_str() == Some(name)) {
        labels.push(Value::String(name.to_string()));
    }
    let mut builder = GroupBuilder::new();
    builder.attributes(attributes);
    let group = builder
        .build(store, labels_group)
        .map_err(|error| error.to_string())?;
    group.store_metadata().map_err(|error| error.to_string())
}

fn spatial_chunks(
    extent: grana_volume::Extent,
    chunk: &[u64],
    x: usize,
    y: usize,
    z: Option<usize>,
) -> usize {
    let spatial = [extent.x(), extent.y(), extent.z()];
    let (depth, depth_extent) = z.map_or((1, 1), |z| (chunk[z], spatial[2]));
    (u64::from(spatial[0]).div_ceil(chunk[x])
        * u64::from(spatial[1]).div_ceil(chunk[y])
        * u64::from(depth_extent).div_ceil(depth)) as usize
}

fn chunk_shape(
    extent: grana_volume::Extent,
    kinds: &[Kind],
    x: usize,
    y: usize,
    z: Option<usize>,
) -> Vec<u64> {
    let mut chunk = vec![1u64; kinds.len()];
    chunk[x] = u64::from(extent.x().min(CHUNK_X));
    chunk[y] = u64::from(extent.y().min(CHUNK_Y));
    if let Some(z) = z {
        chunk[z] = u64::from(extent.z().min(CHUNK_Z));
    }
    chunk
}

fn shape(extent: grana_volume::Extent, frames: u32, kinds: &[Kind]) -> Vec<u64> {
    let spatial = [extent.x(), extent.y(), extent.z()];
    let x = kinds.len() - 1;
    let y = kinds.len() - 2;
    let mut shape = vec![1u64; kinds.len()];
    for (index, kind) in kinds.iter().enumerate() {
        shape[index] = match kind {
            Kind::Time => u64::from(frames),
            Kind::Channel => 1,
            Kind::Space if index == x => u64::from(spatial[0]),
            Kind::Space if index == y => u64::from(spatial[1]),
            Kind::Space => u64::from(spatial[2]),
        };
    }
    shape
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Time,
    Channel,
    Space,
}

fn kind(axis: &Axis) -> Result<Kind, String> {
    match axis.kind() {
        AxisKind::Space => Ok(Kind::Space),
        AxisKind::Time => Ok(Kind::Time),
        AxisKind::Channel => Ok(Kind::Channel),
    }
}
