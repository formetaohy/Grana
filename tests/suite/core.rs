use grana_volume::{
    BrickCoord, BrickGrid, Channel, Extent, Intensity, PADDED_SIDE, Spacing, Volume, VoxelData,
};

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
    assert_eq!(pyramid.channels()[1].window(), Intensity::new(0.0, 1.0));

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
