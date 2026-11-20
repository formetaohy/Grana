use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_archive::{Block, Fill, Filler, ImageSpec, Written};
use grana_volume::{Level, Placement, VoxelData, VoxelFormat};
use serde_json::{Value, json};
use zarrs::array::codec::ZstdCodec;
use zarrs::array::{Array, ArrayBuilder, ArrayBytes, ArraySubset, FillValue, data_type};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::GroupBuilder;

use crate::archive::Archive;
use crate::chunk::Chunk;
use crate::error::Error;
use crate::parallel;
use crate::plan::{Located, Plan};
use crate::shape;

pub(crate) const CANCELLED: &str = "the write was cancelled";
const AXIS_NAMES: [&str; 5] = ["t", "c", "z", "y", "x"];

pub fn write_image(path: &Path, spec: &mut ImageSpec<'_>) -> Result<Written, Error> {
    spec.check();
    let expected =
        crate::space::image_space(spec.levels, spec.frames, spec.channels.len(), spec.sample);
    let mut archive = Archive::create(path, expected)?;
    archive.image(spec)?;
    let bytes = archive.commit()?;
    Ok(Written::new(
        path.to_path_buf(),
        spec.name.to_string(),
        spec.channels.len(),
        spec.frames,
        spec.levels.len(),
        bytes,
    ))
}

pub(crate) fn build(path: &Path, spec: &mut ImageSpec<'_>) -> Result<(), String> {
    assert!(
        spec.interval.is_some(),
        "an OME-Zarr image declares its frame interval"
    );
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
    let sample = spec.sample;
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
            sample_type(sample),
            blank(sample),
        )
        .subchunk_shape(subchunk_shape(plan).to_vec())
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
                    sample,
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
                    sample,
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
    sample: VoxelFormat,
    progress: &mut LevelProgress<'_>,
) -> Result<(), String> {
    let block = plan.block();
    let voxels = block.voxels();
    let report = progress.base;
    let total = progress.total;
    parallel::write_each(
        plan.count(),
        &mut |done, _| (progress.progress)(report + done, total),
        |index| {
            let located = plan.locate(index);
            let data = staged(block, &located, plan.level(), sample, fill)?;
            array
                .store_chunk(
                    &plan.indices(&located),
                    ArrayBytes::new_flen(data.to_le_bytes()),
                )
                .map_err(|error| format!("failed to write a chunk: {error}"))?;
            Ok((voxels * sample.bytes()) as u64)
        },
    )?;
    Ok(())
}

fn derive_level(
    array: &Array<FilesystemStore>,
    plan: &Plan,
    child: &Array<FilesystemStore>,
    source: &Plan,
    sample: VoxelFormat,
    progress: &mut LevelProgress<'_>,
) -> Result<(), String> {
    let block = plan.block();
    let voxels = block.voxels();
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
            let data = derived(block, &located, sample, child, source)?;
            array
                .store_chunk(
                    &plan.indices(&located),
                    ArrayBytes::new_flen(data.to_le_bytes()),
                )
                .map_err(|error| format!("failed to write a chunk: {error}"))?;
            Ok((voxels * sample.bytes()) as u64)
        },
    )?;
    Ok(())
}

fn request(plan: usize, located: &Located) -> Block {
    Block {
        level: plan,
        frame: located.frame,
        channel: located.channel as usize,
        origin: located.origin,
        extent: located.length,
    }
}

fn staged(
    block: Chunk,
    located: &Located,
    level: usize,
    sample: VoxelFormat,
    fill: &Filler<'_>,
) -> Result<VoxelData, String> {
    let extent = block.shape();
    let held = request(level, located);
    let data = fill(held)?;
    held.check(sample, &data);
    let data = data.cast(sample);
    if (0..3).all(|axis| located.length[axis] == extent[axis]) {
        return Ok(data);
    }
    let mut staged = VoxelData::blank(sample, block.voxels());
    staged_into(&mut staged, extent, located.length, &data);
    Ok(staged)
}

fn staged_into(target: &mut VoxelData, extent: [u32; 3], length: [u32; 3], data: &VoxelData) {
    for plane in 0..length[2] {
        target.blit(
            data,
            Placement::new(length[0], [0, plane * length[1]]),
            Placement::new(extent[0], [0, plane * extent[1]]),
            length[0],
            length[1],
        );
    }
}

fn derived(
    block: Chunk,
    located: &Located,
    sample: VoxelFormat,
    child: &Array<FilesystemStore>,
    source: &Plan,
) -> Result<VoxelData, String> {
    let extent = block.shape();
    let count =
        (located.length[0] as usize) * (located.length[1] as usize) * (located.length[2] as usize);
    let mut values = Vec::with_capacity(count);
    reduce(child, source, located, &mut values)?;
    let mut cursor = 0;
    let sampled = VoxelData::from_values(sample, count, || {
        let value = values[cursor];
        cursor += 1;
        value
    });
    if (0..3).all(|axis| located.length[axis] == extent[axis]) {
        return Ok(sampled);
    }
    let mut staged = VoxelData::blank(sample, block.voxels());
    staged_into(&mut staged, extent, located.length, &sampled);
    Ok(staged)
}

fn reduce(
    child: &Array<FilesystemStore>,
    source: &Plan,
    located: &Located,
    values: &mut Vec<f64>,
) -> Result<(), String> {
    let extent = source.extent();
    let width = located.length[0] as usize;
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
    let depth = slab(source.extent());
    let mut z = 0u32;
    while z < located.length[2] {
        let held = depth.min(located.length[2] - z);
        let low = origin[2] + 2 * z;
        let high = (low + 2 * held).min(end[2]);
        let ranges = [
            u64::from(located.frame)..u64::from(located.frame) + 1,
            u64::from(located.channel)..u64::from(located.channel) + 1,
            u64::from(low)..u64::from(high),
            u64::from(origin[1])..u64::from(end[1]),
            u64::from(origin[0])..u64::from(end[0]),
        ];
        let read = retrieve(child, &ArraySubset::new_with_ranges(&ranges))?;
        let depth_z = (high - low) as usize;
        for slice in 0..held {
            for y in 0..located.length[1] {
                let rows = (2 * y)..(2 * y + 2).min(covered[1]);
                for x in 0..located.length[0] {
                    let columns = (2 * x)..(2 * x + 2).min(covered[0]);
                    let mut sum = 0.0f64;
                    let mut count = 0.0f64;
                    for dz in (2 * slice)..((2 * slice + 2).min(depth_z as u32)) {
                        for row in rows.clone() {
                            for column in columns.clone() {
                                let at = column as usize
                                    + covered[0] as usize * (row as usize)
                                    + plane * dz as usize;
                                sum += read.wide_value(at);
                                count += 1.0;
                            }
                        }
                    }
                    debug_assert!(count > 0.0, "a derived voxel averages its children");
                    values.push(sum / count);
                }
            }
        }
        z += held;
    }
    debug_assert_eq!(
        values.len(),
        width * located.length[1] as usize * located.length[2] as usize
    );
    Ok(())
}

fn retrieve(array: &Array<FilesystemStore>, subset: &ArraySubset) -> Result<VoxelData, String> {
    let failed =
        |error: zarrs::array::ArrayError| format!("failed to read the finer level back: {error}");
    let held = array.data_type();
    if held == &data_type::uint8() {
        Ok(VoxelData::U8(
            array
                .retrieve_array_subset::<Vec<u8>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::uint16() {
        Ok(VoxelData::U16(
            array
                .retrieve_array_subset::<Vec<u16>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::uint32() {
        Ok(VoxelData::U32(
            array
                .retrieve_array_subset::<Vec<u32>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::uint64() {
        Ok(VoxelData::U64(
            array
                .retrieve_array_subset::<Vec<u64>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::int8() {
        Ok(VoxelData::I8(
            array
                .retrieve_array_subset::<Vec<i8>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::int16() {
        Ok(VoxelData::I16(
            array
                .retrieve_array_subset::<Vec<i16>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::int32() {
        Ok(VoxelData::I32(
            array
                .retrieve_array_subset::<Vec<i32>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::int64() {
        Ok(VoxelData::I64(
            array
                .retrieve_array_subset::<Vec<i64>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::float32() {
        Ok(VoxelData::F32(
            array
                .retrieve_array_subset::<Vec<f32>>(subset)
                .map_err(failed)?,
        ))
    } else if held == &data_type::float64() {
        Ok(VoxelData::F64(
            array
                .retrieve_array_subset::<Vec<f64>>(subset)
                .map_err(failed)?,
        ))
    } else {
        Err(format!(
            "the finer level stores the unsupported data type `{held}`"
        ))
    }
}

fn slab(extent: [u32; 3]) -> u32 {
    let inner = shape::inner(extent)[2];
    if inner.is_multiple_of(2) {
        inner / 2
    } else {
        inner
    }
}

fn sample_type(sample: VoxelFormat) -> zarrs::array::DataType {
    match sample {
        VoxelFormat::U8 => data_type::uint8(),
        VoxelFormat::U16 => data_type::uint16(),
        VoxelFormat::U32 => data_type::uint32(),
        VoxelFormat::U64 => data_type::uint64(),
        VoxelFormat::I8 => data_type::int8(),
        VoxelFormat::I16 => data_type::int16(),
        VoxelFormat::I32 => data_type::int32(),
        VoxelFormat::I64 => data_type::int64(),
        VoxelFormat::F32 => data_type::float32(),
        VoxelFormat::F64 => data_type::float64(),
    }
}

fn blank(sample: VoxelFormat) -> FillValue {
    match sample {
        VoxelFormat::U8 => FillValue::from(0u8),
        VoxelFormat::U16 => FillValue::from(0u16),
        VoxelFormat::U32 => FillValue::from(0u32),
        VoxelFormat::U64 => FillValue::from(0u64),
        VoxelFormat::I8 => FillValue::from(0i8),
        VoxelFormat::I16 => FillValue::from(0i16),
        VoxelFormat::I32 => FillValue::from(0i32),
        VoxelFormat::I64 => FillValue::from(0i64),
        VoxelFormat::F32 => FillValue::from(0.0f32),
        VoxelFormat::F64 => FillValue::from(0.0f64),
    }
}

fn extent(level: Level) -> [u32; 3] {
    shape::extent(level)
}

fn block(level: Level) -> Chunk {
    Chunk::new(shape::shard(shape::extent(level)))
}

fn chunk_shape(plan: &Plan) -> [u64; 5] {
    let shape = plan.block().shape();
    [
        1,
        1,
        u64::from(shape[2]),
        u64::from(shape[1]),
        u64::from(shape[0]),
    ]
}

fn subchunk_shape(plan: &Plan) -> [u64; 5] {
    let extent = plan.extent();
    let inner = shape::inner(extent);
    [
        1,
        1,
        u64::from(inner[2]),
        u64::from(inner[1]),
        u64::from(inner[0]),
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
    let rotated = crate::schema::rotated(spec.levels[0], 3);
    let schema = crate::schema::Schema::placed(rotated);
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
                f64::from(
                    spec.interval
                        .expect("an OME-Zarr image declares its frame interval"),
                ),
                1.0,
                spacing[2],
                spacing[1],
                spacing[0],
            ];
            let translation = [0.0, 0.0, translation[0], translation[1], translation[2]];
            crate::schema::dataset(schema, &level.to_string(), &scale, &translation)
        })
        .collect::<Vec<_>>();
    let channels = spec
        .channels
        .iter()
        .map(|channel| {
            json!({
                "label": channel.name,
                "color": format!(
                    "{:02X}{:02X}{:02X}",
                    tint(channel.tint[0]),
                    tint(channel.tint[1]),
                    tint(channel.tint[2]),
                ),
                "window": {
                    "min": f64::from(channel.intensity.min()),
                    "max": f64::from(channel.intensity.max()),
                    "start": f64::from(channel.window.min()),
                    "end": f64::from(channel.window.max()),
                },
            })
        })
        .collect::<Vec<_>>();
    let ome = json!({
        "version": schema.version(),
        "multiscales": crate::schema::multiscales(
            schema,
            spec.name,
            &axes,
            datasets,
            rotated.then_some(crate::schema::Display {
                matrix,
                dropped: vec![0, 1],
                spatial: 3,
            }),
        ),
        "omero": { "channels": channels },
    });
    let mut attributes = serde_json::Map::new();
    attributes.insert("ome".to_string(), ome);
    attributes.insert(
        grana_archive::ANNOTATION.to_string(),
        json!({ "calibration": grana_archive::calibration_name(spec.calibration) }),
    );
    attributes
}

fn tint(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
