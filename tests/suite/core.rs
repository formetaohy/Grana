use grana_image::{Image, Source};
use grana_volume::{
    BrickCoord, BrickGrid, Channel, Extent, Intensity, NoData, PADDED_SIDE, Spacing, Volume,
    VolumeSeries, VoxelData,
};
use grana_warp::Warp;

#[test]
fn builds_padded_bricks_from_a_volume() {
    let extent = Extent::new(40, 36, 20);
    let count = extent.voxel_count() as usize;
    let mut first = Vec::with_capacity(count);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                first.push((7 + x + 10 * y + 100 * z) as u16);
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(0.2, 0.2, 0.5),
        vec![
            Channel::new("first", [0.2, 0.5, 1.0], VoxelData::U16(first)),
            Channel::new("second", [1.0, 0.5, 0.2], VoxelData::U16(vec![9; count])),
        ],
    );
    let pyramid = volume.pyramid();
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.level(0).extent(), extent);
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 0.5));
    assert_eq!(pyramid.channels().len(), 2);
    assert_eq!(pyramid.channels()[0].name(), "first");
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(7.0, 2296.0)
    );
    assert_eq!(pyramid.channels()[1].intensity(), Intensity::new(9.0, 9.0));

    let grid = BrickGrid::new(extent);
    assert_eq!(grid.counts(), [2, 2, 1]);
    assert_eq!(grid.count(), 4);
    let brick = volume.brick(&grid, BrickCoord::new(0, 0, 0));
    assert_eq!(brick.channels().len(), 2);
    let VoxelData::U16(values) = &brick.channels()[0] else {
        panic!("the brick keeps the voxel format");
    };
    assert_eq!(
        values.len(),
        (PADDED_SIDE * PADDED_SIDE * PADDED_SIDE) as usize
    );
    let side = PADDED_SIDE as usize;
    let padded = |x: usize, y: usize, z: usize| values[x + side * (y + side * z)];
    assert_eq!(padded(0, 0, 0), 7);
    assert_eq!(padded(1, 1, 1), 7);
    assert_eq!(padded(2, 2, 2), 7 + 1 + 10 + 100);
    assert_eq!(padded(3, 3, 3), 7 + 2 + 20 + 200);
    assert_eq!(padded(33, 33, 21), 7 + 32 + 320 + 1900);
    let VoxelData::U16(second) = &brick.channels()[1] else {
        panic!("the brick keeps the voxel format");
    };
    assert!(second.iter().all(|value| *value == 9));

    let region = volume.channels()[0].data().region(extent, 4..8, 2..5, 1..2);
    let VoxelData::U16(values) = region else {
        panic!("the region keeps the voxel format");
    };
    assert_eq!(values.len(), 4 * 3);
    assert_eq!(values[0], 7 + 4 + 20 + 100);
    assert_eq!(values[values.len() - 1], 7 + 7 + 40 + 100);
}

#[test]
fn grades_missing_voxels_out_of_statistics_and_derived_levels() {
    let extent = Extent::new(64, 64, 64);
    let absent = NoData::F32(-1.0);
    let mut values = (0..extent.voxel_count())
        .map(|index| 2.0 + (index % 1000) as f32 * 1.0e-6)
        .collect::<Vec<f32>>();
    values[0] = -1.0;
    let channel =
        Channel::new("signal", [1.0, 1.0, 1.0], VoxelData::F32(values)).with_no_data(absent);
    let volume = Volume::new(extent, Spacing::new(1.0, 1.0, 1.0), vec![channel]);
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    let info = &image.pyramid().channels()[0];
    assert_eq!(info.no_data(), Some(absent));
    assert_eq!(
        info.statistics().intensity().min(),
        2.0,
        "the census leaves the declared no-data value out"
    );
    assert!(info.statistics().intensity().max() < 2.01);

    let ranges = [60..68, 0..2, 0..2];
    let data = image
        .read_region(0, 0, 0, &ranges, &Warp::IDENTITY)
        .expect("a region across the volume edge reads");
    let VoxelData::F32(read) = data else {
        panic!("the read keeps the voxel format");
    };
    for z in 0..2u32 {
        for y in 0..2u32 {
            for x in 0..8u32 {
                let index = (x + 8 * (y + 2 * z)) as usize;
                let value = read[index];
                if x < 4 {
                    let voxel = (x + 60) as usize + 64 * (y as usize + 64 * z as usize);
                    let expected = 2.0 + (voxel % 1000) as f32 * 1.0e-6;
                    assert_eq!(value, expected, "inside voxel {x},{y},{z}");
                } else {
                    assert_eq!(value, -1.0, "the region outside the extent holds the fill");
                }
            }
        }
    }

    let derived = image
        .read_region(0, 1, 0, &[0..4, 0..4, 0..4], &Warp::IDENTITY)
        .expect("a derived level reads");
    let VoxelData::F32(level) = derived else {
        panic!("the derived level keeps the voxel format");
    };
    assert!(
        (level[0] - 2.0).abs() < 1.0e-3,
        "a reduced voxel averages only the voxels that hold data"
    );
    assert!(
        level.iter().all(|value| (value - 2.0).abs() < 1.0e-3),
        "every reduced voxel leaves the no-data value out"
    );
    assert!(NoData::F32(f32::NAN).matches_sample(f32::NAN));
}

#[test]
fn keeps_a_shifted_region_at_its_intensity_across_the_volume_edge() {
    let extent = Extent::new(32, 32, 16);
    let values = (0..extent.voxel_count())
        .map(|index| 2.0 + (index % 1000) as f32 * 1.0e-6)
        .collect::<Vec<f32>>();
    let channel = Channel::new("signal", [1.0, 1.0, 1.0], VoxelData::F32(values));
    let volume = Volume::new(extent, Spacing::new(1.0, 1.0, 1.0), vec![channel]);
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    let warp = Warp::shift([3.5, 0.0, 0.0]);
    let shifted = image
        .read_region(0, 0, 0, &[28..29, 8..9, 4..5], &warp)
        .expect("a warped voxel reads");
    let source = image
        .read_region(0, 0, 0, &[31..32, 8..9, 4..5], &Warp::IDENTITY)
        .expect("the source voxel reads");
    let (VoxelData::F32(shifted), VoxelData::F32(source)) = (shifted, source) else {
        panic!("the reads keep the voxel format");
    };
    assert_eq!(
        shifted[0], source[0],
        "a half covered tap renormalises instead of dimming the edge voxel"
    );
    let beyond = image
        .read_region(0, 0, 0, &[29..32, 8..9, 4..5], &warp)
        .expect("a region past the source edge reads");
    let VoxelData::F32(beyond) = beyond else {
        panic!("the read keeps the voxel format");
    };
    assert!(
        beyond.iter().all(|value| *value == 0.0),
        "a voxel whose taps all leave the volume holds the fill"
    );
}
