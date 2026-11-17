use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_archive::{Fill, Filler, ImageSpec, Written};
use grana_volume::Level;
use serde_json::{Value, json};
use zarrs::array::codec::ZstdCodec;
use zarrs::array::{Array, ArrayBuilder, ArraySubset, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::GroupBuilder;

use crate::chunk::Chunk;
use crate::error::Error;
use crate::parallel;
use crate::plan::{Located, Plan};

const CHUNK_X: u32 = 256;
const CHUNK_Y: u32 = 256;
const CHUNK_Z: u32 = 32;
const SLAB_Z: u32 = 4;
const CANCELLED: &str = "the write was cancelled";
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
    build(atomic.staging(), spec).map_err(|detail| Error::Write {
        path: path.display().to_string(),
        detail,
    })?;
    let bytes = grana_file::bytes(atomic.staging()).map_err(|error| Error::Write {
        path: path.display().to_string(),
        detail: format!("failed to measure the written dataset: {error}"),
    })?;
    if spec.cancelled() {
        return Err(Error::Write {
            path: path.display().to_string(),
            detail: CANCELLED.to_string(),
        });
    }
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

fn build(path: &Path, spec: &mut ImageSpec<'_>) -> Result<(), String> {
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
    let frames = spec.frames;
    let channels = spec.channels.len();
    let fill = spec.fill;
    let cancel = spec.cancel;
    let plans = spec
        .levels
        .iter()
        .enumerate()
        .map(|(level, held)| Plan::new(level, extent(*held), block(*held), frames, channels))
        .collect::<Vec<_>>();
    let total = plans.iter().map(Plan::count).sum::<usize>();
    let progress: &mut dyn FnMut(usize, usize) = &mut *spec.progress;
    let mut base = 0;
    let mut previous: Option<Array<FilesystemStore>> = None;
    for (level, plan) in plans.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.to_string());
        }
        let array = ArrayBuilder::new(
            shape(spec.levels[level], frames, channels),
            chunk_shape(plan).to_vec(),
            data_type::float32(),
            0.0f32,
        )
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
        match (fill, level) {
            (Fill::EveryLevel(filler), _) | (Fill::FinestLevel(filler), 0) => {
                fill_level(
                    &array,
                    plan,
                    filler,
                    &mut LevelProgress {
                        base,
                        total,
                        cancel,
                        progress,
                    },
                )?;
            }
            (Fill::FinestLevel(_), _) => {
                let child = previous
                    .as_ref()
                    .expect("a derived level follows its written child");
                derive_level(
                    &array,
                    plan,
                    child,
                    &plans[level - 1],
                    &mut LevelProgress {
                        base,
                        total,
                        cancel,
                        progress,
                    },
                )?;
            }
        }
        base += plan.count();
        previous = Some(array);
    }
    Ok(())
}

struct LevelProgress<'a> {
    base: usize,
    total: usize,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(usize, usize),
}

fn fill_level(
    array: &Array<FilesystemStore>,
    plan: &Plan,
    fill: &Filler<'_>,
    progress: &mut LevelProgress<'_>,
) -> Result<(), String> {
    let block = plan.block();
    let voxels = block.voxels();
    let shape = block.shape();
    let channels = plan.channels();
    let report = progress.base;
    let total = progress.total;
    parallel::write_each(
        plan.count(),
        &mut |done, _| (progress.progress)(report + done, total),
        |index| {
            let located = plan.locate(index);
            let full = located.length == shape;
            let mut data = vec![0.0f32; voxels * channels];
            for channel in 0..channels {
                let target = &mut data[channel * voxels..(channel + 1) * voxels];
                if full {
                    fill(
                        plan.level(),
                        located.frame,
                        channel,
                        located.origin,
                        located.length,
                        target,
                    )?;
                    continue;
                }
                let length = located.length;
                let mut region =
                    vec![0.0f32; length[0] as usize * length[1] as usize * length[2] as usize];
                fill(
                    plan.level(),
                    located.frame,
                    channel,
                    located.origin,
                    length,
                    &mut region,
                )?;
                block.stage(length, &region, target);
            }
            array
                .store_chunk(&plan.indices(&located), data)
                .map_err(|error| format!("failed to write a chunk: {error}"))?;
            Ok((voxels * channels * size_of::<f32>()) as u64)
        },
    )?;
    Ok(())
}

fn derive_level(
    array: &Array<FilesystemStore>,
    plan: &Plan,
    child: &Array<FilesystemStore>,
    source: &Plan,
    progress: &mut LevelProgress<'_>,
) -> Result<(), String> {
    let block = plan.block();
    let voxels = block.voxels();
    let channels = plan.channels();
    let report = progress.base;
    let total = progress.total;
    let cancel = progress.cancel;
    parallel::write_each(
        plan.count(),
        &mut |done, _| (progress.progress)(report + done, total),
        |index| {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.to_string());
            }
            let located = plan.locate(index);
            let mut data = vec![0.0f32; voxels * channels];
            for channel in 0..channels {
                let target = &mut data[channel * voxels..(channel + 1) * voxels];
                let region = reduce(child, source, &located, channel)?;
                block.stage(located.length, &region, target);
            }
            array
                .store_chunk(&plan.indices(&located), data)
                .map_err(|error| format!("failed to write a chunk: {error}"))?;
            Ok((voxels * channels * size_of::<f32>()) as u64)
        },
    )?;
    Ok(())
}

fn reduce(
    child: &Array<FilesystemStore>,
    source: &Plan,
    located: &Located,
    channel: usize,
) -> Result<Vec<f32>, String> {
    let extent = source.extent();
    let width = located.length[0] as usize;
    let line = located.length[1] as usize;
    let mut region = vec![0.0f32; width * line * located.length[2] as usize];
    let origin = [
        2 * located.origin[0],
        2 * located.origin[1],
        2 * located.origin[2],
    ];
    let end = [
        (2 * (located.origin[0] + located.length[0])).min(extent[0]),
        (2 * (located.origin[1] + located.length[1])).min(extent[1]),
        (2 * (located.origin[2] + located.length[2])).min(extent[2]),
    ];
    let covered = [end[0] - origin[0], end[1] - origin[1], end[2] - origin[2]];
    let plane = covered[0] as usize * covered[1] as usize;
    let mut z = 0u32;
    while z < located.length[2] {
        let depth = SLAB_Z.min(located.length[2] - z);
        let low = origin[2] + 2 * z;
        let high = (low + 2 * depth).min(end[2]);
        let ranges = [
            u64::from(located.frame)..u64::from(located.frame) + 1,
            channel as u64..channel as u64 + 1,
            u64::from(low)..u64::from(high),
            u64::from(origin[1])..u64::from(end[1]),
            u64::from(origin[0])..u64::from(end[0]),
        ];
        let held = child
            .retrieve_array_subset::<Vec<f32>>(&ArraySubset::new_with_ranges(&ranges))
            .map_err(|error| format!("failed to read the finer level back: {error}"))?;
        let slab = (high - low) as usize;
        for slice in 0..depth {
            for y in 0..located.length[1] {
                let rows = (2 * y)..(2 * y + 2).min(covered[1]);
                for x in 0..located.length[0] {
                    let columns = (2 * x)..(2 * x + 2).min(covered[0]);
                    let mut sum = 0.0f64;
                    let mut count = 0.0f64;
                    for dz in (2 * slice)..((2 * slice + 2).min(slab as u32)) {
                        for row in rows.clone() {
                            for column in columns.clone() {
                                let at = column as usize
                                    + covered[0] as usize * (row as usize)
                                    + plane * dz as usize;
                                sum += f64::from(held[at]);
                                count += 1.0;
                            }
                        }
                    }
                    debug_assert!(count > 0.0, "a derived voxel averages its children");
                    region[x as usize + width * (y as usize + line * (z + slice) as usize)] =
                        (sum / count) as f32;
                }
            }
        }
        z += depth;
    }
    Ok(region)
}

fn extent(level: Level) -> [u32; 3] {
    let extent = level.extent();
    [extent.x(), extent.y(), extent.z()]
}

fn block(level: Level) -> Chunk {
    let extent = level.extent();
    Chunk::new([
        extent.x().min(CHUNK_X),
        extent.y().min(CHUNK_Y),
        extent.z().min(CHUNK_Z),
    ])
}

fn chunk_shape(plan: &Plan) -> [u64; 5] {
    let shape = plan.block().shape();
    [
        1,
        plan.channels() as u64,
        u64::from(shape[2]),
        u64::from(shape[1]),
        u64::from(shape[0]),
    ]
}

fn shape(level: Level, frames: u32, channels: usize) -> Vec<u64> {
    let extent = level.extent();
    vec![
        u64::from(frames),
        channels as u64,
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
