use std::f64::consts::PI;

use glam::Vec3;
use grana_analysis::{SurfaceBuilder, SurfaceParams};
use grana_volume::{
    BrickData, BrickGrid, Channel, Extent, Intensity, Level, Region, Spacing, Volume, VoxelData,
};

const EXTENT: Extent = Extent::new(120, 96, 72);
const RADIUS: f32 = 12.0;
const HIGH: u16 = 65_535;
const MARKER: u16 = 7;

#[test]
fn extracts_measured_surfaces() {
    let centres = [[36.0f32, 48.0, 36.0], [84.0, 40.0, 30.0]];
    let volume = blobs(&centres);
    let level = Level::new(volume.extent(), volume.spacing());
    let grid = BrickGrid::new(volume.extent());
    let threshold = 0.5;
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();

    let mut builder = SurfaceBuilder::new(
        0,
        level,
        SurfaceParams::new(threshold, 0.0),
        channels,
        0,
        Region::none(),
        intensity,
    );
    for index in 0..builder.coord_count() {
        let coord = builder.coord(index);
        builder.add(coord, &brick(&volume, &grid, coord), None);
    }
    let set = builder.finish();
    assert_eq!(set.object_count(), centres.len(), "each blob is one object");
    assert!(!set.geometry().is_empty(), "the mesh holds triangles");
    assert_eq!(set.intensities().channels(), channels);

    let spacing = [0.1f64, 0.1, 0.2];
    let radius = f64::from(RADIUS) * (spacing[0] * spacing[1] * spacing[2]).cbrt();
    let expected_volume = 4.0 / 3.0 * PI * radius.powi(3);
    let expected_area = 4.0 * PI * radius.powi(2);
    let world = level.world_extent();
    let expected_centres = centres.map(|centre| {
        [0, 1, 2].map(|axis| (centre[axis] + 0.5) * spacing[axis] as f32 - 0.5 * world[axis])
    });

    for (index, object) in set.objects().iter().enumerate() {
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

        let detection = set.intensities().object(index, 0);
        assert!(
            (detection.mean() - f32::from(HIGH)).abs() < 1.0,
            "the detection channel inside a blob is saturated"
        );
        let marker = set.intensities().object(index, 1);
        assert!(
            (marker.mean() - f32::from(MARKER)).abs() < 1.0e-6,
            "the marker channel is measured inside the object"
        );
        assert!(
            (marker.sum() - f64::from(MARKER) * object.voxel_count() as f64).abs() < 1.0,
            "the marker sum covers every object voxel"
        );
        assert_eq!(marker.deviation(), 0.0, "a constant channel is flat");
    }

    let mut filtered = SurfaceBuilder::new(
        0,
        level,
        SurfaceParams::new(threshold, (1.5 * expected_volume) as f32),
        channels,
        0,
        Region::none(),
        intensity,
    );
    for index in 0..filtered.coord_count() {
        let coord = filtered.coord(index);
        filtered.add(coord, &brick(&volume, &grid, coord), None);
    }
    let filtered = filtered.finish();
    assert!(
        filtered.is_empty(),
        "the volume filter must drop small objects"
    );
    assert_eq!(filtered.intensities().objects(), 0);

    let region = Region::new(Vec3::new(-6.0, -4.8, -7.2), Vec3::new(0.5, 4.8, 7.2));
    let mut restricted = SurfaceBuilder::new(
        0,
        level,
        SurfaceParams::new(threshold, 0.0),
        channels,
        0,
        region,
        intensity,
    );
    for index in 0..restricted.coord_count() {
        let coord = restricted.coord(index);
        restricted.add(coord, &brick(&volume, &grid, coord), None);
    }
    let restricted = restricted.finish();
    assert_eq!(
        restricted.object_count(),
        1,
        "the region keeps a single blob"
    );
    assert!(
        restricted.objects()[0].centroid()[0] < 0.0,
        "the kept blob lies inside the region"
    );
    assert!(
        restricted.geometry().triangle_count() > 0,
        "the restricted surface carries triangles"
    );
}

fn brick(volume: &Volume, grid: &BrickGrid, coord: grana_volume::BrickCoord) -> BrickData {
    BrickData::new(
        (0..volume.channel_count())
            .map(|channel| volume.channel_brick(grid, coord, channel))
            .collect(),
    )
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
        vec![
            Channel::new("blobs", [0.3, 0.8, 1.0], VoxelData::U16(values)),
            Channel::new(
                "marker",
                [0.8, 0.3, 0.3],
                VoxelData::U16(vec![MARKER; count]),
            ),
        ],
    )
}

fn spacing() -> Spacing {
    Spacing::new(0.1, 0.1, 0.2)
}
