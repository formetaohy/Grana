use std::sync::Arc;

use grana_image::{Image, Source};
use grana_optics::PsfParams;
use grana_pipeline::Channels;
use grana_scene::{Dataset, RestoreSpec, Scene};
use grana_volume::{
    BrickCoord, Channel, Extent, Intensity, Spacing, Volume, VolumeSeries, VoxelData,
};

const EXTENT: Extent = Extent::new(24, 24, 8);

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

fn scene(reader: &Image) -> Scene {
    let mut scene = Scene::new(Dataset::synthetic("restore", reader.pyramid().clone()));
    let params = PsfParams::new(1.4, 0.52, 1.515, 1.33, 0.2);
    let spec = RestoreSpec::new(
        0,
        "dapi restored",
        [0.0, 0.0, 1.0],
        Intensity::new(0.0, 20000.0),
        params,
        8,
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
    let scene = scene(&reader);
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
