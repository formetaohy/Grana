use std::f64::consts::PI;

use grana_analysis::{SurfaceBuilder, SurfaceParams};
use grana_volume::{BrickGrid, Channel, Extent, Level, Spacing, Volume, VoxelData};

const EXTENT: Extent = Extent::new(120, 96, 72);
const RADIUS: f32 = 12.0;
const HIGH: u16 = 65_535;

#[test]
fn extracts_measured_surfaces() {
    let centres = [[36.0f32, 48.0, 36.0], [84.0, 40.0, 30.0]];
    let volume = blobs(&centres);
    let level = Level::new(volume.extent(), volume.spacing());
    let grid = BrickGrid::new(volume.extent());
    let threshold = 0.5 * f32::from(HIGH);

    let mut builder = SurfaceBuilder::new(0, 0, level, SurfaceParams::new(threshold, 0.0));
    for index in 0..builder.coord_count() {
        let coord = builder.coord(index);
        let values = volume.channel_brick(&grid, coord, 0);
        builder.add(coord, &values);
    }
    let set = builder.finish();
    assert_eq!(set.object_count(), centres.len(), "each blob is one object");
    assert!(!set.mesh().is_empty(), "the mesh holds triangles");

    let spacing = [0.1f64, 0.1, 0.2];
    let radius = f64::from(RADIUS) * (spacing[0] * spacing[1] * spacing[2]).cbrt();
    let expected_volume = 4.0 / 3.0 * PI * radius.powi(3);
    let expected_area = 4.0 * PI * radius.powi(2);
    let world = level.world_extent();
    let expected_centres = centres.map(|centre| {
        [0, 1, 2].map(|axis| (centre[axis] + 0.5) * spacing[axis] as f32 - 0.5 * world[axis])
    });

    for object in set.objects() {
        let relative = (object.volume() - expected_volume).abs() / expected_volume;
        assert!(
            relative < 0.08,
            "a blob volume {} is off from {expected_volume}",
            object.volume()
        );
        let relative = (object.area() - expected_area).abs() / expected_area;
        assert!(
            relative < 0.25,
            "a blob area {} is off from {expected_area}",
            object.area()
        );
        assert!(
            (0.7..=1.0).contains(&object.sphericity()),
            "a sphere is not spherical: {}",
            object.sphericity()
        );
        let centre = object.centroid();
        let placed = expected_centres.iter().any(|candidate| {
            (0..3)
                .map(|axis| f64::from(centre[axis] - candidate[axis]).powi(2))
                .sum::<f64>()
                < 0.25
        });
        assert!(placed, "the object centroid {centre:?} is misplaced");
        let bounds = object.bounds();
        assert!(bounds.contains(centre), "the centroid lies in the bounds");
    }

    let mut filtered = SurfaceBuilder::new(
        0,
        0,
        level,
        SurfaceParams::new(threshold, (1.5 * expected_volume) as f32),
    );
    for index in 0..filtered.coord_count() {
        let coord = filtered.coord(index);
        let values = volume.channel_brick(&grid, coord, 0);
        filtered.add(coord, &values);
    }
    assert!(
        filtered.finish().is_empty(),
        "the volume filter must drop small objects"
    );
}

fn blobs(centres: &[[f32; 3]]) -> Volume {
    let count = EXTENT.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for centre in centres {
        for z in 0..EXTENT.z() {
            for y in 0..EXTENT.y() {
                for x in 0..EXTENT.x() {
                    let offset = ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt();
                    if offset < RADIUS {
                        values[EXTENT.index(x, y, z) as usize] = HIGH;
                    }
                }
            }
        }
    }
    Volume::new(
        EXTENT,
        spacing(),
        vec![Channel::new(
            "blobs",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn spacing() -> Spacing {
    Spacing::new(0.1, 0.1, 0.2)
}
