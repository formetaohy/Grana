use std::path::PathBuf;

use grana_image::{Format, Image, Source, is_image};
use grana_volume::{
    BrickCoord, Channel, Extent, PADDED_SIDE, Spacing, Volume, VolumeSeries, VoxelData,
};

#[test]
fn opens_a_tiff_file_through_the_source_layer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plane.tif");
    assert!(is_image(&path));
    let image = Image::open(&Source::file(path)).expect("the tiff opens");
    assert_eq!(image.format(), Some(Format::Tiff));
    assert_eq!(image.name(), "plane");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let data = image
        .read_region(0, 0, 0, &[0..4, 0..3, 0..1])
        .expect("the plane reads");
    let VoxelData::U8(values) = data else {
        panic!("the fixture stores bytes");
    };
    for y in 0..3u32 {
        for x in 0..4u32 {
            assert_eq!(values[(y * 4 + x) as usize], (x + 10 * y) as u8);
        }
    }
}

#[test]
fn resolves_coarser_levels_from_the_stored_level() {
    let extent = Extent::new(64, 64, 8);
    let mut samples = Vec::with_capacity(extent.voxel_count() as usize);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                samples.push((4 * x + 8 * y + 16 * z) as u16);
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 0.0, 0.0],
            VoxelData::U16(samples),
        )],
    );
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    assert_eq!(image.pyramid().stored(), 1);
    assert_eq!(image.pyramid().level_count(), 2);
    assert_eq!(image.pyramid().level(1).extent(), Extent::new(32, 32, 4));
    let resolved = image
        .read_region(0, 1, 0, &[0..32, 0..32, 0..4])
        .expect("the resolved level reads");
    let VoxelData::U16(values) = resolved else {
        panic!("the resolved level keeps the voxel format");
    };
    for z in 0..4u32 {
        for y in 0..32u32 {
            for x in 0..32u32 {
                let expected = (8 * x + 16 * y + 32 * z + 14) as u16;
                assert_eq!(values[(x + 32 * (y + 32 * z)) as usize], expected);
            }
        }
    }
    let brick = image
        .read_brick(0, 1, BrickCoord::new(0, 0, 0))
        .expect("the resolved brick reads");
    let VoxelData::U16(samples) = &brick.channels()[0] else {
        panic!("the resolved brick keeps the voxel format");
    };
    let side = PADDED_SIDE as usize;
    assert_eq!(samples.len(), side * side * side);
    assert_eq!(samples[side * (1 + side)], 14);
    assert_eq!(samples[side * side * side - 1], 854);
    assert!(image.read_region(0, 2, 0, &[0..1, 0..1, 0..1]).is_err());
}
