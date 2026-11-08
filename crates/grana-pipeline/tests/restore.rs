use std::sync::Arc;

use grana_image::{Image, Source};
use grana_optics::{KernelBank, PsfParams};
use grana_pipeline::Channels;
use grana_restore::Restorer;
use grana_scene::{Dataset, RestoreSpec, Scene};
use grana_volume::{
    BrickCoord, Channel, Extent, Intensity, Spacing, Volume, VolumeSeries, VoxelData,
};

const EXTENT: Extent = Extent::new(24, 24, 8);
const ITERATIONS: u32 = 4;

fn series() -> VolumeSeries {
    let mut data = vec![0u16; EXTENT.voxel_count() as usize];
    let centre = [EXTENT.x() / 2, EXTENT.y() / 2, EXTENT.z() / 2];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let offset = [
                    x as f32 - centre[0] as f32,
                    y as f32 - centre[1] as f32,
                    z as f32 - centre[2] as f32,
                ];
                let distance = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
                if distance <= 2.0 {
                    let index = EXTENT.index(x, y, z) as usize;
                    data[index] = 4000;
                }
            }
        }
    }
    let volume = Volume::new(
        EXTENT,
        Spacing::new(0.1, 0.1, 0.3),
        vec![Channel::new("dapi", [0.0, 0.0, 1.0], VoxelData::U16(data))],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn blob_series() -> VolumeSeries {
    let extent = Extent::new(130, 40, 6);
    let mut data = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let left = gaussian(x as f32, y as f32, 64.0, 20.0, 12.0);
                let right = gaussian(x as f32, y as f32, 110.0, 20.0, 6.0);
                let value = (9000.0 * left.max(right)).round();
                let index = extent.index(x, y, z) as usize;
                data[index] = value as u16;
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(0.05, 0.05, 0.3),
        vec![Channel::new("dapi", [0.0, 0.0, 1.0], VoxelData::U16(data))],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn gaussian(x: f32, y: f32, at_x: f32, at_y: f32, sigma: f32) -> f32 {
    let dx = x - at_x;
    let dy = y - at_y;
    (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp()
}

fn params() -> PsfParams {
    PsfParams::new(1.4, 0.52, 1.515, 1.33, 0.2)
}

fn scene(reader: &Image, iterations: u32) -> Scene {
    let mut scene = Scene::new(Dataset::synthetic("restore", reader.pyramid().clone()));
    let spec = RestoreSpec::new(
        0,
        "dapi restored",
        [0.0, 0.0, 1.0],
        Intensity::new(0.0, 20000.0),
        params(),
        iterations,
    );
    let channel = scene
        .add_restore(spec)
        .expect("the restored channel is added");
    assert_eq!(channel, 1);
    scene
}

fn values(data: &VoxelData) -> &[f32] {
    let VoxelData::F32(values) = data else {
        panic!("the restored channel reads as floats");
    };
    values
}

#[test]
fn a_restored_channel_reads_regions_and_bricks_consistently() {
    let reader = Arc::new(Image::open(&Source::memory(series())).expect("the volume opens"));
    let scene = scene(&reader, 8);
    let channels = Channels::new(Arc::clone(&reader), &scene);
    let extent = reader.pyramid().level(0).extent();
    let whole = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let restored = channels
        .read_region(0, 0, 1, &whole, [0.0; 3])
        .expect("the restored channel reads");
    let values = values(&restored);
    assert_eq!(
        values.len() as u64,
        extent.voxel_count(),
        "the restored channel covers every voxel"
    );
    assert!(
        values
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0),
        "richardson-lucy keeps every voxel finite and non negative"
    );
    let stored = channels
        .read_region(0, 0, 0, &whole, [0.0; 3])
        .expect("the stored channel reads");
    let VoxelData::U16(stored) = &stored else {
        panic!("the stored channel keeps its integer format");
    };
    let source: f32 = stored.iter().map(|value| f32::from(*value)).sum();
    let total: f32 = values.iter().sum();
    assert!(
        (total - source).abs() < source * 0.2,
        "the restoration keeps {total} of {source} photons"
    );
    let brick = channels
        .read_brick(0, 0, BrickCoord::new(0, 0, 0), [0.0; 3])
        .expect("the restored brick reads");
    assert_eq!(brick.channels().len(), 2);
    let padded = values_of(&brick.channels()[1]);
    let grid = grana_volume::BrickGrid::new(extent);
    let mut compared = 0;
    for z in 0..grana_volume::PADDED_SIDE {
        for y in 0..grana_volume::PADDED_SIDE {
            for x in 0..grana_volume::PADDED_SIDE {
                let cell = grid.padded_cell(BrickCoord::new(0, 0, 0), [x, y, z]);
                let region = extent.index(cell[0], cell[1], cell[2]) as usize;
                let brick =
                    (x + grana_volume::PADDED_SIDE * (y + grana_volume::PADDED_SIDE * z)) as usize;
                assert!(
                    (padded[brick] - (values[region])).abs()
                        <= f32::EPSILON.max(values[region].abs() * 1.0e-5),
                    "the brick agrees with the region at {cell:?}"
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 0, "the brick covers the volume");
}

fn values_of(data: &VoxelData) -> &[f32] {
    let VoxelData::F32(values) = data else {
        panic!("the restored brick reads as floats");
    };
    values
}

#[test]
fn a_restored_channel_blends_its_tiles() {
    let reader = Arc::new(Image::open(&Source::memory(blob_series())).expect("the volume opens"));
    let scene = scene(&reader, ITERATIONS);
    let channels = Channels::new(Arc::clone(&reader), &scene);
    let level = reader.pyramid().level(0);
    let extent = level.extent();
    let read = |x: std::ops::Range<u32>| {
        channels
            .read_region(0, 0, 1, &[x, 0..extent.y(), 0..extent.z()], [0.0; 3])
            .expect("the restored range reads")
    };
    let whole = read(0..extent.x());
    let whole = values(&whole);
    let left = read(0..70);
    let left = values(&left);
    let right = read(60..extent.x());
    let right = values(&right);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..70 {
                let blended = whole[extent.index(x, y, z) as usize];
                let split = left[(x + 70 * (y + 40 * z)) as usize];
                assert!(
                    (blended - split).abs() <= blended.abs().max(1.0) * 1.0e-5,
                    "the left read matches the whole read at ({x}, {y}, {z})"
                );
                let offset = x + 60;
                let blended = whole[extent.index(offset, y, z) as usize];
                let split = right[(x + 70 * (y + 40 * z)) as usize];
                assert!(
                    (blended - split).abs() <= blended.abs().max(1.0) * 1.0e-5,
                    "the right read matches the whole read at ({offset}, {y}, {z})"
                );
            }
        }
    }
    let bank = KernelBank::new(params(), level.spacing(), extent.z());
    let stored = reader
        .read_region(
            0,
            0,
            0,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
            [0.0; 3],
        )
        .expect("the stored channel reads");
    let mut source = vec![0.0f32; extent.voxel_count() as usize];
    stored.write_values(&mut source);
    let whole_volume = Restorer::shared().restore(
        [extent.x(), extent.y(), extent.z()],
        &source,
        &bank,
        0,
        ITERATIONS,
    );
    let peak = whole_volume.iter().copied().fold(0.0f32, f32::max);
    let deviation = whole
        .iter()
        .zip(&whole_volume)
        .map(|(tiled, whole)| (tiled - whole).abs())
        .fold(0.0f32, f32::max);
    assert!(
        deviation <= peak * 0.01,
        "the stitched tiles approach a whole volume restoration by {deviation} of {peak}"
    );
}
