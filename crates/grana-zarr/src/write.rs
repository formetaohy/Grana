use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{Intensity, Level};
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

pub type ChunkFill<'a> =
    dyn FnMut(usize, u32, usize, [u32; 3], [u32; 3], &mut [f32]) -> Result<(), String> + 'a;

pub struct WriteChannel<'a> {
    pub name: &'a str,
    pub tint: [f32; 3],
    pub range: Intensity,
}

pub struct ImageSpec<'a> {
    pub name: &'a str,
    pub channels: &'a [WriteChannel<'a>],
    pub levels: &'a [Level],
    pub placement: [f32; 3],
    pub frames: u32,
    pub interval: f32,
    pub fill: &'a mut ChunkFill<'a>,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

pub struct ImageWritten {
    path: PathBuf,
    name: String,
    channels: usize,
    frames: u32,
    levels: usize,
    bytes: u64,
}

impl ImageWritten {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn levels(&self) -> usize {
        self.levels
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

pub fn write_image(path: &Path, mut spec: ImageSpec<'_>) -> Result<ImageWritten, Error> {
    assert!(
        !spec.name.trim().is_empty(),
        "an exported image carries a name"
    );
    assert!(
        !spec.channels.is_empty(),
        "an exported image carries at least one channel"
    );
    assert!(
        spec.channels
            .iter()
            .all(|channel| !channel.name.trim().is_empty()),
        "every exported channel carries a name"
    );
    assert!(
        !spec.levels.is_empty(),
        "an exported image carries at least one level"
    );
    assert!(spec.frames > 0, "an exported image carries a frame");
    assert!(
        spec.interval.is_finite() && spec.interval > 0.0,
        "an exported image carries a positive frame interval"
    );
    assert!(
        spec.placement.iter().all(|value| value.is_finite()),
        "an exported image carries a finite placement"
    );
    if path.is_file() {
        return Err(Error::Write {
            path: path.display().to_string(),
            detail: "the destination is a file".to_string(),
        });
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| Error::Write {
        path: path.display().to_string(),
        detail: format!("failed to create {}: {error}", parent.display()),
    })?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| Error::Write {
            path: path.display().to_string(),
            detail: "the destination names no folder".to_string(),
        })?;
    let staging = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|error| Error::Write {
            path: path.display().to_string(),
            detail: format!("failed to clear {}: {error}", staging.display()),
        })?;
    }
    let outcome = build(&staging, &mut spec);
    let bytes = match outcome {
        Ok(bytes) => bytes,
        Err(detail) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(Error::Write {
                path: path.display().to_string(),
                detail,
            });
        }
    };
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| Error::Write {
            path: path.display().to_string(),
            detail: format!("failed to replace {}: {error}", path.display()),
        })?;
    }
    std::fs::rename(&staging, path).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging);
        Error::Write {
            path: path.display().to_string(),
            detail: format!("failed to move the dataset into place: {error}"),
        }
    })?;
    Ok(ImageWritten {
        path: path.to_path_buf(),
        name: spec.name.to_string(),
        channels: spec.channels.len(),
        frames: spec.frames,
        levels: spec.levels.len(),
        bytes,
    })
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
    let datasets = spec
        .levels
        .iter()
        .enumerate()
        .map(|(level, value)| {
            let spacing = value.spacing();
            let origin = value.origin();
            let scale = [spacing.x(), spacing.y(), spacing.z()].map(f64::from);
            let translation = [0, 1, 2].map(|axis| {
                f64::from(spec.placement[axis]) + f64::from(origin[axis]) + 0.5 * scale[axis]
            });
            let scale = [f64::from(spec.interval), 1.0, scale[2], scale[1], scale[0]];
            let translation = [0.0, 0.0, translation[2], translation[1], translation[0]];
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
        "multiscales": crate::schema::multiscales(spec.name, &axes, datasets),
        "omero": { "channels": channels },
    });
    let mut attributes = serde_json::Map::new();
    attributes.insert("ome".to_string(), ome);
    attributes
}

fn tint(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
