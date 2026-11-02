use std::f64::consts::PI;

use glam::Vec3;
use grana_analysis::{SurfaceBuilder, SurfaceParams};
use grana_object::SurfaceSet;
use grana_volume::{Channel, Extent, Intensity, Level, Region, Spacing, Volume, VoxelData};

const HIGH: u16 = 65_535;
const MARKER: u16 = 7;

#[test]
fn extracts_measured_surfaces() {
    let extent = Extent::new(120, 96, 72);
    let centres = [[36.0f32, 48.0, 36.0], [84.0, 40.0, 30.0]];
    let volume = blobs(extent, &[0.1, 0.1, 0.2], &centres, 12.0);
    let level = Level::new(volume.extent(), volume.spacing());
    let threshold = 0.5;
    let channels = volume.channel_count();

    let set = extract(
        &volume,
        SurfaceParams::new(threshold, 0.0, 0.0, 0.0),
        Region::none(),
    );
    assert_eq!(set.object_count(), centres.len(), "each blob is one object");
    assert!(!set.geometry().is_empty(), "the mesh holds triangles");
    assert_eq!(set.intensities().channels(), channels);

    let spacing = [0.1f64, 0.1, 0.2];
    let radius = 12.0 * (spacing[0] * spacing[1] * spacing[2]).cbrt();
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

    let filtered = extract(
        &volume,
        SurfaceParams::new(threshold, 0.0, 0.0, (1.5 * expected_volume) as f32),
        Region::none(),
    );
    assert!(
        filtered.is_empty(),
        "the volume filter must drop small objects"
    );
    assert_eq!(filtered.intensities().objects(), 0);

    let smoothed = extract(
        &volume,
        SurfaceParams::new(threshold, 0.1, 0.0, 0.0),
        Region::none(),
    );
    assert_eq!(
        smoothed.object_count(),
        centres.len(),
        "the grain keeps every blob"
    );
    for object in smoothed.objects() {
        let relative = (object.volume() - expected_volume).abs() / expected_volume;
        assert!(
            relative < 0.08,
            "the grain shifts a blob volume {} from {expected_volume}",
            object.volume()
        );
    }

    let region = Region::new(Vec3::new(-6.0, -4.8, -7.2), Vec3::new(0.5, 4.8, 7.2));
    let restricted = extract(
        &volume,
        SurfaceParams::new(threshold, 0.0, 0.0, 0.0),
        region,
    );
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

#[test]
fn splits_touching_lobes_at_their_neck() {
    let extent = Extent::new(200, 80, 80);
    let radius = 10.0;
    let centres = [[86.5f32, 40.0, 40.0], [105.5, 40.0, 40.0]];
    let volume = blobs(extent, &[0.1, 0.1, 0.1], &centres, radius);

    let merged = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0),
        Region::none(),
    );
    assert_eq!(merged.object_count(), 1, "touching lobes form one object");
    let total = merged.objects()[0].voxel_count();

    let split = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.6, 0.0),
        Region::none(),
    );
    assert_eq!(
        split.object_count(),
        2,
        "the seed splits the lobes at their neck"
    );
    let kept = split
        .objects()
        .iter()
        .map(|object| object.voxel_count())
        .sum::<u64>();
    assert_eq!(kept, total, "splitting conserves the mask");

    let expected = 4.0 / 3.0 * PI * f64::from(radius).powi(3);
    for object in split.objects() {
        let relative = (object.voxel_count() as f64 - expected).abs() / expected;
        assert!(
            relative < 0.03,
            "a lobe holds {} voxels against {expected}",
            object.voxel_count()
        );
    }
    let mut centres = split
        .objects()
        .iter()
        .map(|object| object.centroid()[0])
        .collect::<Vec<_>>();
    centres.sort_by(|left, right| left.partial_cmp(right).expect("the centroids order"));
    let separation = centres[1] - centres[0];
    assert!(
        (1.5..2.3).contains(&separation),
        "the lobes are {separation} apart in x"
    );

    let coarse = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 2.4, 0.0),
        Region::none(),
    );
    assert_eq!(
        coarse.object_count(),
        1,
        "a seed wider than the neck keeps the lobes together"
    );
}

fn extract(volume: &Volume, params: SurfaceParams, region: Region) -> SurfaceSet {
    let level = Level::new(volume.extent(), volume.spacing());
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();
    let mut builder = SurfaceBuilder::new(0, level, params, channels, 0, region, intensity);
    for index in 0..builder.tile_count() {
        let range = builder.detection_range(index);
        let detection = volume.channels()[0].data().region(
            volume.extent(),
            range[0].clone(),
            range[1].clone(),
            range[2].clone(),
        );
        builder
            .add(index, &detection, None, |channel, core| {
                Ok(volume.channels()[channel].data().region(
                    volume.extent(),
                    core[0].clone(),
                    core[1].clone(),
                    core[2].clone(),
                ))
            })
            .expect("a surface tile is added");
    }
    builder.finish()
}

fn blobs(extent: Extent, spacing: &[f32; 3], centres: &[[f32; 3]], radius: f32) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for centre in centres {
        for z in 0..extent.z() {
            for y in 0..extent.y() {
                for x in 0..extent.x() {
                    let offset = ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt();
                    if offset < radius {
                        values[extent.index(x, y, z) as usize] = HIGH;
                    }
                }
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
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
