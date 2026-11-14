use std::path::Path;
use std::sync::Arc;

use grana_archive::{ImageSpec, Written};
use grana_volume::Level;
use serde_json::{Value, json};
use zarrs::array::codec::ZstdCodec;
use zarrs::array::{Array, ArrayBuilder, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::GroupBuilder;

use crate::error::Error;

const CHUNK_X: u32 = 256;
const CHUNK_Y: u32 = 256;
const CHUNK_Z: u32 = 32;
const AXIS_NAMES: [&str; 5] = ["t", "c", "z", "y", "x"];

pub fn write_image(path: &Path, spec: &mut ImageSpec<'_>) -> Result<Written, Error> {
    spec.check();
    if path.is_file() {
        return Err(Error::Write {
            path: path.display().to_string(),
            detail: "the destination is a file".to_string(),
        });
    }
    let atomic = grana_file::Atomic::directory(path).map_err(|error| Error::Write {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;
    let bytes = build(atomic.staging(), spec).map_err(|detail| Error::Write {
        path: path.display().to_string(),
        detail,
    })?;
    atomic.commit().map_err(|error| Error::Write {
        path: path.display().to_string(),
        detail: format!("failed to move the dataset into place: {error}"),
    })?;
    Ok(Written::new(
        path.to_path_buf(),
        spec.name.to_string(),
        spec.channels.len(),
        spec.frames,
        spec.levels.len(),
        bytes,
    ))
}

fn build(path: &Path, spec: &mut ImageSpec<'_>) -> Result<u64, String> {
    let store = Arc::new(
        FilesystemStore::new(path).map_err(|error| format!("failed to open the store: {error}"))?,
    );
    let mut builder = GroupBuilder::new();
    builder.attributes(attributes(spec));
    let group = builder
        .build(store.clone(), "/")
        .map_err(|error| format!("failed to write the root group: {error}"))?;
    group
        .store_metadata()
        .map_err(|error| format!("failed to write the root metadata: {error}"))?;
    let total = (0..spec.levels.len())
        .map(|level| chunks(spec.levels[level]) as usize * spec.frames as usize)
        .sum::<usize>();
    let mut stats = Stats {
        done: 0,
        total,
        bytes: 0,
    };
    for level in 0..spec.levels.len() {
        let shape = shape(spec.levels[level], spec);
        let chunk = chunk_shape(spec.levels[level], spec.channels.len());
        let array = ArrayBuilder::new(shape, chunk.clone(), data_type::float32(), 0.0f32)
            .bytes_to_bytes_codecs(vec![Arc::new(ZstdCodec::new(3, false))])
            .dimension_names(Some(
                AXIS_NAMES
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect::<Vec<_>>(),
            ))
            .build(store.clone(), &format!("/{level}"))
            .map_err(|error| format!("failed to build level {level}: {error}"))?;
        array
            .store_metadata()
            .map_err(|error| format!("failed to write the metadata of level {level}: {error}"))?;
        write_level(&array, spec, level, &chunk, &mut stats)?;
    }
    Ok(stats.bytes)
}

struct Stats {
    done: usize,
    total: usize,
    bytes: u64,
}

fn write_level<S>(
    array: &Array<S>,
    spec: &mut ImageSpec<'_>,
    level: usize,
    chunk: &[u64],
    stats: &mut Stats,
) -> Result<(), String>
where
    S: ?Sized + zarrs::storage::WritableStorageTraits + 'static,
{
    let extent = spec.levels[level].extent();
    let spatial = [extent.x(), extent.y(), extent.z()];
    let counts = [
        u64::from(spatial[0]).div_ceil(chunk[4]),
        u64::from(spatial[1]).div_ceil(chunk[3]),
        u64::from(spatial[2]).div_ceil(chunk[2]),
    ];
    let channels = spec.channels.len();
    let chunk_voxels = (chunk[2] * chunk[3] * chunk[4]) as usize;
    let mut indices = vec![0u64; AXIS_NAMES.len()];
    for frame in 0..spec.frames {
        for cz in 0..counts[2] {
            for cy in 0..counts[1] {
                for cx in 0..counts[0] {
                    let origin = [
                        (cx * chunk[4]) as u32,
                        (cy * chunk[3]) as u32,
                        (cz * chunk[2]) as u32,
                    ];
                    let length = [
                        chunk[4].min(u64::from(spatial[0] - origin[0])) as u32,
                        chunk[3].min(u64::from(spatial[1] - origin[1])) as u32,
                        chunk[2].min(u64::from(spatial[2] - origin[2])) as u32,
                    ];
                    let mut data = vec![0.0f32; chunk_voxels * channels];
                    let full = length[0] as usize == chunk[4] as usize
                        && length[1] as usize == chunk[3] as usize
                        && length[2] as usize == chunk[2] as usize;
                    for channel in 0..channels {
                        let base = channel * chunk_voxels;
                        if full {
                            (spec.fill)(
                                level,
                                frame,
                                channel,
                                origin,
                                length,
                                &mut data[base..base + chunk_voxels],
                            )?;
                            continue;
                        }
                        let voxels = length[0] as usize * length[1] as usize * length[2] as usize;
                        let mut region = vec![0.0f32; voxels];
                        (spec.fill)(level, frame, channel, origin, length, &mut region)?;
                        let width = length[0] as usize;
                        let row = chunk[4] as usize;
                        let depth_stride = row * chunk[3] as usize;
                        for z in 0..length[2] as usize {
                            for y in 0..length[1] as usize {
                                let from = width * (y + length[1] as usize * z);
                                let to = base + row * y + depth_stride * z;
                                data[to..to + width].copy_from_slice(&region[from..from + width]);
                            }
                        }
                    }
                    indices[0] = u64::from(frame);
                    indices[1] = 0;
                    indices[2] = cz;
                    indices[3] = cy;
                    indices[4] = cx;
                    array
                        .store_chunk(&indices, data)
                        .map_err(|error| format!("failed to write a chunk: {error}"))?;
                    stats.bytes += (chunk_voxels * channels * size_of::<f32>()) as u64;
                    stats.done += 1;
                    (spec.progress)(stats.done, stats.total);
                }
            }
        }
    }
    Ok(())
}

fn chunks(level: Level) -> u64 {
    let extent = level.extent();
    let chunk = chunk_shape(level, 1);
    u64::from(extent.x()).div_ceil(chunk[4])
        * u64::from(extent.y()).div_ceil(chunk[3])
        * u64::from(extent.z()).div_ceil(chunk[2])
}

fn chunk_shape(level: Level, channels: usize) -> Vec<u64> {
    let extent = level.extent();
    vec![
        1,
        channels as u64,
        u64::from(extent.z().min(CHUNK_Z)),
        u64::from(extent.y().min(CHUNK_Y)),
        u64::from(extent.x().min(CHUNK_X)),
    ]
}

fn shape(level: Level, spec: &ImageSpec<'_>) -> Vec<u64> {
    let extent = level.extent();
    vec![
        u64::from(spec.frames),
        spec.channels.len() as u64,
        u64::from(extent.z()),
        u64::from(extent.y()),
        u64::from(extent.x()),
    ]
}

fn attributes(spec: &ImageSpec<'_>) -> serde_json::Map<String, Value> {
    let axes = crate::schema::axes();
    let matrix = crate::schema::placement(spec.levels[0], 3);
    let inverse = crate::schema::inverse(matrix);
    let display = crate::schema::rotated(spec.levels[0], 3);
    let datasets = spec
        .levels
        .iter()
        .enumerate()
        .map(|(level, value)| {
            let basis = value.basis();
            let spacing = [basis.step(0), basis.step(1), basis.step(2)].map(f64::from);
            let center = value.voxel_center([0.0, 0.0, 0.0]);
            let point =
                [0, 1, 2].map(|axis| f64::from(spec.placement[axis]) + f64::from(center[axis]));
            let translation = [0, 1, 2].map(|row| {
                (0..3)
                    .map(|column| inverse[row][column] * point[column])
                    .sum::<f64>()
            });
            let scale = [
                f64::from(spec.interval),
                1.0,
                spacing[2],
                spacing[1],
                spacing[0],
            ];
            let translation = [0.0, 0.0, translation[0], translation[1], translation[2]];
            crate::schema::dataset(&level.to_string(), &scale, &translation)
        })
        .collect::<Vec<_>>();
    let channels = spec
        .channels
        .iter()
        .map(|channel| {
            let range = channel.range;
            json!({
                "label": channel.name,
                "color": format!(
                    "{:02X}{:02X}{:02X}",
                    tint(channel.tint[0]),
                    tint(channel.tint[1]),
                    tint(channel.tint[2]),
                ),
                "window": {
                    "min": f64::from(range.min()),
                    "max": f64::from(range.max()),
                    "start": f64::from(range.min()),
                    "end": f64::from(range.max()),
                },
            })
        })
        .collect::<Vec<_>>();
    let ome = json!({
        "version": crate::schema::VERSION,
        "multiscales": crate::schema::multiscales(
            spec.name,
            &axes,
            datasets,
            display.then_some(crate::schema::Display {
                matrix,
                dropped: vec![0, 1],
                spatial: 3,
            }),
        ),
        "omero": { "channels": channels },
    });
    let mut attributes = serde_json::Map::new();
    attributes.insert("ome".to_string(), ome);
    attributes
}

fn tint(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
