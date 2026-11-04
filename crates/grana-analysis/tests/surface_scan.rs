use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::{Sources, SurfaceBuilder, coloc_levels, filament_level, recut, surface_level};
use grana_geometry::GeometryStore;
use grana_object::SurfaceSet;
use grana_object::{SurfaceFilter, SurfaceParams};
use grana_volume::{
    Channel, ChannelInfo, Extent, Intensity, Level, Pyramid, Region, Spacing, Volume, VoxelData,
};

const HIGH: u16 = 65_535;
const MARKER: u16 = 7;

fn params(threshold: f32) -> SurfaceParams {
    SurfaceParams::new(
        threshold,
        0.0,
        0.0,
        0.0,
        SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
    )
}

#[test]
fn extracts_measured_surfaces() {
    let extent = Extent::new(120, 96, 72);
    let centres = [[36.0f32, 48.0, 36.0], [84.0, 40.0, 30.0]];
    let volume = blobs(extent, &[0.1, 0.1, 0.2], &centres, 12.0);
    let level = Level::new(volume.extent(), volume.spacing());
    let threshold = 0.5;
    let channels = volume.channel_count();

    let set = extract(&volume, params(threshold), Region::none());
    assert_eq!(set.object_count(), centres.len(), "each blob is one object");
    assert!(
        !set.geometry().expect("the mesh loads").is_empty(),
        "the mesh holds triangles"
    );
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
        SurfaceParams::new(
            threshold,
            0.0,
            0.0,
            (1.5 * expected_volume) as f32,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        ),
        Region::none(),
    );
    assert!(
        filtered.is_empty(),
        "the volume filter must drop small objects"
    );
    assert_eq!(filtered.intensities().objects(), 0);

    let smoothed = extract(
        &volume,
        SurfaceParams::new(
            threshold,
            0.1,
            0.0,
            0.0,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        ),
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
    let restricted = extract(&volume, params(threshold), region);
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
        restricted
            .geometry()
            .expect("the mesh loads")
            .triangle_count()
            > 0,
        "the restricted surface carries triangles"
    );
}

#[test]
fn splits_touching_lobes_at_their_neck() {
    let extent = Extent::new(200, 80, 80);
    let radius = 10.0;
    let centres = [[86.5f32, 40.0, 40.0], [105.5, 40.0, 40.0]];
    let volume = blobs(extent, &[0.1, 0.1, 0.1], &centres, radius);

    let merged = extract(&volume, params(0.5), Region::none());
    assert_eq!(merged.object_count(), 1, "touching lobes form one object");
    let total = merged.objects()[0].voxel_count();

    let split = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.3, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(
        split.object_count(),
        2,
        "the split separates the lobes at their neck"
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
    let capped = split
        .objects()
        .iter()
        .map(|object| object.area())
        .sum::<f64>();
    assert!(
        capped > merged.objects()[0].area(),
        "the split lobes carry their neck caps: {capped} against {}",
        merged.objects()[0].area()
    );
    assert!(watertight(&split), "each split lobe is a closed body");

    let coarse = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.8, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(
        coarse.object_count(),
        1,
        "a split fraction deeper than the neck keeps the lobes together"
    );
}

#[test]
fn cuts_touching_lobes_at_the_seed_points() {
    let extent = Extent::new(200, 80, 80);
    let spacing = [0.1f32, 0.1, 0.1];
    let centres = [[86.5f32, 40.0, 40.0], [105.5, 40.0, 40.0]];
    let volume = blobs(extent, &spacing, &centres, 10.0);
    let level = Level::new(volume.extent(), volume.spacing());
    let world = level.world_extent();
    let merged = extract(&volume, params(0.5), Region::none());
    assert_eq!(merged.object_count(), 1, "the lobes form one object");
    let total = merged.objects()[0].voxel_count();

    let seeds = centres.map(|centre| {
        [0, 1, 2].map(|axis| (centre[axis] + 0.5) * spacing[axis] - 0.5 * world[axis])
    });
    let set = extract_seeded(&volume, params(0.5), &seeds)
        .expect("the seeded surface is detected")
        .expect("surface detection completes");
    assert_eq!(set.object_count(), 2, "the seed points cut the neck");
    let kept = set
        .objects()
        .iter()
        .map(|object| object.voxel_count())
        .sum::<u64>();
    assert_eq!(kept, total, "the seeded cut conserves the mask");
    assert!(watertight(&set), "each seeded lobe is a closed body");
}

#[test]
fn rejects_a_seed_point_outside_the_surface() {
    let extent = Extent::new(120, 96, 72);
    let spacing = [0.1f32, 0.1, 0.2];
    let volume = blobs(extent, &spacing, &[[36.0, 48.0, 36.0]], 12.0);
    let level = Level::new(volume.extent(), volume.spacing());
    let world = level.world_extent();
    let seed = [0, 1, 2].map(|axis| (100.0f32 + 0.5) * spacing[axis] - 0.5 * world[axis]);
    let failure = extract_seeded(&volume, params(0.5), &[seed])
        .expect_err("a seed point outside the surface is rejected");
    assert!(failure.contains("seed point"), "{failure}");
}

#[test]
fn splits_large_lobes_at_their_thick_neck() {
    let volume = large_lobes();
    let filter = || SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let merged = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, filter()),
        Region::none(),
    );
    assert_eq!(merged.object_count(), 1, "touching lobes form one object");
    let total = merged.objects()[0].voxel_count();

    let split = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.3, 0.0, filter()),
        Region::none(),
    );
    assert_eq!(
        split.object_count(),
        2,
        "a neck far wider than the base distance limit still splits"
    );
    let kept = split
        .objects()
        .iter()
        .map(|object| object.voxel_count())
        .sum::<u64>();
    assert_eq!(kept, total, "splitting conserves the mask");

    let mut centres = split
        .objects()
        .iter()
        .map(|object| object.centroid()[0])
        .collect::<Vec<_>>();
    centres.sort_by(|left, right| left.partial_cmp(right).expect("the centroids order"));
    let separation = (centres[1] - centres[0]) / volume.spacing().x();
    assert!(
        (30.0..46.0).contains(&separation),
        "the lobes are {separation} voxels apart in x"
    );
    assert!(
        watertight(&split),
        "each thick necked lobe is a closed body"
    );
}

#[test]
fn recuts_a_detection_without_reading_the_volume() {
    let volume = large_lobes();
    let filter = SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let params = SurfaceParams::new(0.5, 0.0, 0.7, 0.0, filter);
    let set = extract(&volume, params, Region::none());
    assert_eq!(
        set.object_count(),
        1,
        "the shallow split keeps the lobes joined"
    );
    let mut recut_params = params;
    recut_params.set_split(0.3);
    let cut = recut(&set, recut_params.cut(), 2, None)
        .expect("the carried topology is cut")
        .expect("a detected surface carries its topology");
    let fresh = extract(&volume, recut_params, Region::none());
    assert_eq!(cut, fresh, "a recut matches a detection at the same split");
}

#[test]
fn recuts_around_the_objects_a_user_deleted() {
    let volume = large_lobes();
    let filter = SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let params = SurfaceParams::new(0.5, 0.0, 0.3, 0.0, filter);
    let split = extract(&volume, params, Region::none());
    assert_eq!(
        split.object_count(),
        2,
        "the deep split separates the lobes"
    );
    let pruned = split
        .without(0)
        .expect("the packed mesh prunes its object")
        .expect("one lobe remains");
    assert_eq!(pruned.object_count(), 1);
    assert!(
        pruned.cuttable(),
        "a hand deleted object keeps the watershed topology"
    );
    let mut joined = params;
    joined.set_split(0.7);
    assert_eq!(
        extract(&volume, joined, Region::none()).object_count(),
        1,
        "the shallow split joins the lobes"
    );
    let cut = recut(&pruned, joined.cut(), 2, None)
        .expect("the carried topology is cut")
        .expect("a hand edited surface keeps its topology");
    assert_eq!(
        cut.object_count(),
        1,
        "the deleted lobe never merges back into the kept lobe"
    );
    assert_eq!(cut.removed(), pruned.removed());
}

#[test]
fn recuts_a_detection_whose_topology_spilled_to_disk() {
    let volume = large_lobes();
    let filter = SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let params = SurfaceParams::new(0.5, 0.0, 0.7, 0.0, filter);
    let store = GeometryStore::temporary(1).expect("the store opens a spill file");
    let set = extract_through(&volume, params, Region::none(), 2, &store);
    assert_eq!(set.object_count(), 1, "the shallow split keeps the lobes");
    assert!(store.stats().spilled() > 0, "the topology leaves memory");
    let mut recut_params = params;
    recut_params.set_split(0.3);
    let cut = recut(&set, recut_params.cut(), 2, Some(&store))
        .expect("the spilled topology is cut")
        .expect("a detected surface carries its topology");
    let fresh = extract(&volume, recut_params, Region::none());
    assert_eq!(
        cut, fresh,
        "a recut from the spill file matches a fresh detection"
    );
}

#[test]
fn recuts_objects_that_a_filter_dropped() {
    let volume = large_lobes();
    let filter = SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let params = SurfaceParams::new(0.5, 0.0, 0.3, 1.0e6, filter);
    let set = extract(&volume, params, Region::none());
    assert!(set.is_empty(), "the volume filter drops every lobe");
    let mut open = params;
    open.set_min_volume(0.0);
    let cut = recut(&set, open.cut(), 2, None)
        .expect("the carried topology is cut")
        .expect("a filtered surface keeps its topology");
    assert_eq!(cut.object_count(), 2, "the filter revival splits the lobes");
}

#[test]
fn recuts_an_unsplit_detection_to_a_deeper_split() {
    let volume = large_lobes();
    let filter = SurfaceFilter::new(0.0, 0.8, 0.0, 0.0);
    let merged = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, filter),
        Region::none(),
    );
    assert_eq!(merged.object_count(), 1, "touching lobes form one object");
    assert!(
        merged.cuttable(),
        "an unsplit detection still carries its watershed seams"
    );
    let mut split = SurfaceParams::new(0.5, 0.0, 0.3, 0.0, filter);
    let cut = recut(&merged, split.cut(), 2, None)
        .expect("the carried topology is cut")
        .expect("a detected surface carries its topology");
    assert_eq!(
        cut.object_count(),
        2,
        "the deeper split separates the lobes without reading the volume again"
    );
    split.set_split(0.0);
    let restored = recut(&merged, split.cut(), 2, None)
        .expect("the carried topology is cut")
        .expect("a detected surface carries its topology");
    assert_eq!(
        restored.object_count(),
        1,
        "the shallow split joins the lobes again"
    );
}

#[test]
fn keeps_surfaces_deterministic_across_worker_counts() {
    let volume = large_lobes();
    let surface = SurfaceParams::new(0.5, 0.0, 0.6, 0.0, SurfaceFilter::new(0.0, 0.8, 0.0, 0.0));
    let single = extract_with(&volume, surface, Region::none(), 1);
    let many = extract_with(&volume, surface, Region::none(), 4);
    assert_eq!(
        single, many,
        "the surface set does not depend on the worker count"
    );
}

#[test]
fn detects_objects_on_an_uneven_background() {
    let extent = Extent::new(240, 80, 80);
    let centres = [[60.0f32, 40.0, 40.0], [190.0, 40.0, 40.0]];
    let volume = illuminated(extent, &[0.1, 0.1, 0.1], &centres, 10.0, 0.2, 0.8, 0.35);
    let set = extract(&volume, params(0.25), Region::none());
    assert_eq!(
        set.object_count(),
        centres.len(),
        "every blob survives the illumination ramp"
    );
    let expected = 4.0 / 3.0 * PI * 10.0f64.powi(3);
    for object in set.objects() {
        let relative = (object.voxel_count() as f64 - expected).abs() / expected;
        assert!(
            relative < 0.1,
            "a blob under the ramp holds {} voxels against {expected}",
            object.voxel_count()
        );
    }
}

#[test]
fn fills_cavities_enclosed_by_a_surface() {
    let extent = Extent::new(96, 96, 96);
    let volume = shell(extent, &[0.1, 0.1, 0.1], [48.0, 48.0, 48.0], 20.0, 10.0);
    let set = extract(&volume, params(0.5), Region::none());
    assert_eq!(set.object_count(), 1, "the shell is one object");
    let expected = 4.0 / 3.0 * PI * 20.0f64.powi(3);
    let relative = (set.objects()[0].voxel_count() as f64 - expected).abs() / expected;
    assert!(
        relative < 0.05,
        "the cavity is filled: {} voxels against {expected}",
        set.objects()[0].voxel_count()
    );
}

#[test]
fn fills_a_cavity_spanning_tiles() {
    let extent = Extent::new(240, 140, 140);
    let volume = shell(extent, &[0.1, 0.1, 0.1], [120.0, 70.0, 70.0], 45.0, 30.0);
    let set = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.01, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(set.object_count(), 1, "the shell stays one object");
    let expected = 4.0 / 3.0 * PI * 45.0f64.powi(3);
    assert!(
        watertight(&set),
        "the cross tile surface is watertight on every axis"
    );
    let relative = (set.objects()[0].voxel_count() as f64 - expected).abs() / expected;
    assert!(
        relative < 0.05,
        "the cross tile cavity is filled: {} voxels against {expected}",
        set.objects()[0].voxel_count()
    );
}

#[test]
fn keeps_a_large_flat_topped_object_solid() {
    let extent = Extent::new(200, 200, 200);
    let volume = block(extent, &[0.1, 0.1, 0.1], 21, 180);
    let set = extract(&volume, params(0.5), Region::none());
    assert_eq!(set.object_count(), 1, "the block is one object");
    let expected = 159.0f64.powi(3);
    let relative = (set.objects()[0].voxel_count() as f64 - expected).abs() / expected;
    assert!(
        relative < 0.03,
        "the block is solid: {} voxels against {expected}",
        set.objects()[0].voxel_count()
    );
    assert!(
        watertight(&set),
        "the block surface is watertight on every axis"
    );

    let region = Region::new(Vec3::new(-10.0, -5.0, -5.0), Vec3::new(10.0, 5.0, 5.0));
    let cut = extract(&volume, params(0.5), region);
    assert_eq!(cut.object_count(), 1, "the cut block stays one object");
    let expected = 159.0 * 100.0 * 100.0;
    let relative = (cut.objects()[0].voxel_count() as f64 - expected).abs() / expected;
    assert!(
        relative < 0.03,
        "the cut block is solid: {} voxels against {expected}",
        cut.objects()[0].voxel_count()
    );
    assert!(watertight(&cut), "the region cut is capped without holes");
}

type Corner = [u32; 3];
type EdgeKey = (Corner, Corner);
type EdgeCounts = std::collections::HashMap<EdgeKey, u32>;

fn watertight(set: &SurfaceSet) -> bool {
    edge_counts(set)
        .iter()
        .all(|edges| edges.values().all(|count| *count == 2))
}
fn no_holes(set: &SurfaceSet) -> bool {
    edge_counts(set)
        .iter()
        .all(|edges| edges.values().all(|count| *count != 1))
}

fn edge_counts(set: &SurfaceSet) -> Vec<EdgeCounts> {
    let geometry = set.geometry().expect("the mesh loads");
    geometry
        .groups()
        .iter()
        .enumerate()
        .map(|(object, group)| {
            let mut edges: EdgeCounts = std::collections::HashMap::new();
            for index in 0..group.index_count() as usize / 3 {
                let corners = geometry
                    .triangle(object, index)
                    .map(|point| point.map(f32::to_bits));
                for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
                    let first = corners[pair[0]];
                    let second = corners[pair[1]];
                    assert_ne!(first, second, "a triangle edge holds a positive length");
                    let key = if first < second {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    *edges.entry(key).or_insert(0) += 1;
                }
            }
            edges
        })
        .collect()
}

#[test]
fn detects_dim_objects_under_a_scattering_step() {
    let extent = Extent::new(320, 96, 96);
    let centres = [[60.0f32, 48.0, 48.0], [260.0, 48.0, 48.0]];
    let volume = scattering(extent, &[0.1, 0.1, 0.1], &centres, 10.0);
    let set = extract(&volume, params(0.25), Region::none());
    assert_eq!(
        set.object_count(),
        centres.len(),
        "the scattering step neither floods nor hides the dim objects"
    );
    let expected = 4.0 / 3.0 * PI * 10.0f64.powi(3);
    for object in set.objects() {
        let relative = (object.voxel_count() as f64 - expected).abs() / expected;
        assert!(
            relative < 0.15,
            "a dim object holds {} voxels against {expected}",
            object.voxel_count()
        );
    }
}

#[test]
fn keeps_dim_objects_beside_a_saturated_block() {
    let extent = Extent::new(192, 128, 128);
    let volume = saturated(extent, &[0.1, 0.1, 0.1]);
    let set = extract(&volume, params(0.5), Region::none());
    assert_eq!(
        set.object_count(),
        2,
        "the bright plateau and the dim object are both objects"
    );
    let mut volumes = set
        .objects()
        .iter()
        .map(|object| object.voxel_count())
        .collect::<Vec<_>>();
    volumes.sort_unstable();
    let expected = 4.0 / 3.0 * PI * 8.0f64.powi(3);
    let relative = (volumes[0] as f64 - expected).abs() / expected;
    assert!(
        relative < 0.2,
        "the dim object holds {} voxels against {expected}",
        volumes[0]
    );
}

#[test]
fn removes_noise_and_seals_cracks_with_morphology() {
    let extent = Extent::new(160, 96, 96);
    let volume = bridged(extent, &[0.1, 0.1, 0.1]);
    let coarse = extract(&volume, params(0.5), Region::none());
    assert_eq!(
        coarse.object_count(),
        3,
        "the spheres and the noise speck are separate objects"
    );
    let opened = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.2, 0.0, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(opened.object_count(), 2, "opening removes the noise speck");
    let sealed = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.2, 0.2, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(
        sealed.object_count(),
        1,
        "closing seals the one voxel crack"
    );
}

#[test]
fn filters_objects_by_shape_and_intensity() {
    let extent = Extent::new(240, 120, 120);
    let volume = mixed(extent, &[0.1, 0.1, 0.1]);
    let set = extract(&volume, params(0.25), Region::none());
    assert_eq!(set.object_count(), 2, "the fixture holds two objects");
    let round = extract(
        &volume,
        SurfaceParams::new(0.25, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.85, 0.0)),
        Region::none(),
    );
    assert_eq!(round.object_count(), 1, "only the sphere is spherical");
    assert_eq!(round.geometry().expect("the mesh loads").object_count(), 1);
    assert_eq!(round.intensities().objects(), 1);
    let bright = extract(
        &volume,
        SurfaceParams::new(0.25, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.75)),
        Region::none(),
    );
    assert_eq!(bright.object_count(), 1, "only the bright object passes");
    assert_eq!(bright.geometry().expect("the mesh loads").object_count(), 1);
    assert_eq!(bright.intensities().objects(), 1);
    assert_eq!(bright.geometry().expect("the mesh loads").groups().len(), 1);
}

#[test]
fn measures_a_solid_object_through_its_interior() {
    let extent = Extent::new(160, 160, 160);
    let volume = blobs(extent, &[0.1, 0.1, 0.1], &[[80.0, 80.0, 80.0]], 40.0);
    let set = extract(&volume, params(0.5), Region::none());
    assert_eq!(set.object_count(), 1, "a uniform sphere is one object");
    let expected = 4.0 / 3.0 * PI * 40.0f64.powi(3);
    let object = set.objects()[0];
    let relative = (object.voxel_count() as f64 - expected).abs() / expected;
    assert!(
        relative < 0.02,
        "the sphere is solid: {} voxels against {expected}",
        object.voxel_count()
    );
    let marker = set.intensities().object(0, 1);
    assert!(
        (marker.mean() - f32::from(MARKER)).abs() < 1.0e-6,
        "the marker channel is measured through the interior: {}",
        marker.mean()
    );
    assert!(
        (marker.sum() - f64::from(MARKER) * object.voxel_count() as f64).abs() < 1.0,
        "every interior voxel is measured"
    );
}

#[test]
fn splits_solid_lobes_at_their_neck() {
    let extent = Extent::new(96, 96, 96);
    let centres = [[30.0f32, 48.0, 48.0], [65.0, 48.0, 48.0]];
    let volume = blobs(extent, &[0.1, 0.1, 0.1], &centres, 25.0);
    let merged = extract(&volume, params(0.5), Region::none());
    assert_eq!(merged.object_count(), 1, "touching lobes form one object");
    let split = extract(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.2, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Region::none(),
    );
    assert_eq!(
        split.object_count(),
        2,
        "a split shallower than the neck splits the solid lobes"
    );
    let total = split
        .objects()
        .iter()
        .map(|object| object.voxel_count())
        .sum::<u64>();
    assert_eq!(
        total,
        merged.objects()[0].voxel_count(),
        "splitting conserves the mask"
    );
    assert!(
        no_holes(&split),
        "each split solid lobe is closed at its neck"
    );
}

#[test]
fn caps_a_surface_cut_by_the_region() {
    let extent = Extent::new(160, 160, 160);
    let volume = blobs(extent, &[0.1, 0.1, 0.1], &[[80.0, 80.0, 80.0]], 20.0);
    let region = Region::new(Vec3::new(0.0, -8.0, -8.0), Vec3::new(8.0, 8.0, 8.0));
    let set = extract(&volume, params(0.5), region);
    assert_eq!(set.object_count(), 1, "the cut blob stays one object");
    let object = set.objects()[0];
    let half = 0.5 * 4.0 / 3.0 * PI * 20.0f64.powi(3);
    let relative = (object.voxel_count() as f64 - half).abs() / half;
    assert!(
        relative < 0.1,
        "the region keeps half a sphere: {} against {half}",
        object.voxel_count()
    );
    assert!(
        (0.78..=0.90).contains(&object.sphericity()),
        "the capped half sphere is a closed body: {}",
        object.sphericity()
    );
    assert!(watertight(&set), "the region cut is capped without holes");
}

#[test]
fn picks_the_finest_level_that_fits_each_analysis_budget() {
    let mut levels = vec![Level::new(
        Extent::new(4096, 4096, 1024),
        Spacing::new(0.05, 0.05, 0.2),
    )];
    for _ in 0..5 {
        levels.push(levels.last().copied().expect("the level exists").coarser());
    }
    let pyramid = Pyramid::new(
        levels,
        1,
        vec![ChannelInfo::new(
            "signal",
            [1.0, 0.0, 0.0],
            Intensity::new(0.0, f32::from(HIGH)),
            Intensity::new(0.0, f32::from(HIGH)),
            grana_volume::VoxelFormat::U16,
        )],
        1,
        1.0,
    );
    assert_eq!(
        surface_level(&pyramid, Region::none()),
        Some(2),
        "the surfaces halve the stored level until the voxel budget fits"
    );
    assert_eq!(
        filament_level(&pyramid),
        Some(3),
        "the filaments pick the synthesized level that fits their budget"
    );
    assert_eq!(
        coloc_levels(&pyramid, Region::none(), 1).scan(),
        4,
        "the colocalization scans the first level within its budget"
    );
}

fn extract(volume: &Volume, params: SurfaceParams, region: Region) -> SurfaceSet {
    extract_with(volume, params, region, 1)
}

fn extract_seeded(
    volume: &Volume,
    params: SurfaceParams,
    seeds: &[[f32; 3]],
) -> Result<Option<SurfaceSet>, String> {
    let level = Level::new(volume.extent(), volume.spacing());
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();
    let builder = SurfaceBuilder::new(0, level, params, channels, 0, Region::none(), intensity)
        .with_seeds(seeds);
    let cancel = AtomicBool::new(false);
    builder.scan(
        1,
        &cancel,
        None,
        &mut |_done, _total| {},
        Sources::new(
            &|_index, ranges| {
                Ok(volume.channels()[0].data().region(
                    volume.extent(),
                    ranges[0].clone(),
                    ranges[1].clone(),
                    ranges[2].clone(),
                ))
            },
            &|channel, core| {
                Ok(volume.channels()[channel].data().region(
                    volume.extent(),
                    core[0].clone(),
                    core[1].clone(),
                    core[2].clone(),
                ))
            },
            &GeometryStore::memory(),
        ),
    )
}

fn extract_with(
    volume: &Volume,
    params: SurfaceParams,
    region: Region,
    workers: usize,
) -> SurfaceSet {
    let store = GeometryStore::memory();
    extract_through(volume, params, region, workers, &store)
}

fn extract_through(
    volume: &Volume,
    params: SurfaceParams,
    region: Region,
    workers: usize,
    store: &Arc<GeometryStore>,
) -> SurfaceSet {
    let level = Level::new(volume.extent(), volume.spacing());
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();
    let builder = SurfaceBuilder::new(0, level, params, channels, 0, region, intensity);
    let cancel = AtomicBool::new(false);
    builder
        .scan(
            workers,
            &cancel,
            None,
            &mut |_done, _total| {},
            Sources::new(
                &|_index, ranges| {
                    Ok(volume.channels()[0].data().region(
                        volume.extent(),
                        ranges[0].clone(),
                        ranges[1].clone(),
                        ranges[2].clone(),
                    ))
                },
                &|channel, core| {
                    Ok(volume.channels()[channel].data().region(
                        volume.extent(),
                        core[0].clone(),
                        core[1].clone(),
                        core[2].clone(),
                    ))
                },
                store,
            ),
        )
        .expect("a surface frame is detected")
        .expect("surface detection completes")
}

fn large_lobes() -> Volume {
    let extent = Extent::new(200, 80, 80);
    let centres = [[80.0f32, 40.0, 40.0], [118.0, 40.0, 40.0]];
    blobs(extent, &[0.1, 0.1, 0.1], &centres, 22.0)
}

fn blobs(extent: Extent, spacing: &[f32; 3], centres: &[[f32; 3]], radius: f32) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let inside = centres.iter().any(|centre| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                        < radius
                });
                if inside {
                    values[extent.index(x, y, z) as usize] = HIGH;
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

fn block(extent: Extent, spacing: &[f32; 3], low: u32, high: u32) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                if x >= low && x < high && y >= low && y < high && z >= low && z < high {
                    values[extent.index(x, y, z) as usize] = HIGH;
                }
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "block",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn illuminated(
    extent: Extent,
    spacing: &[f32; 3],
    centres: &[[f32; 3]],
    radius: f32,
    low: f32,
    high: f32,
    contrast: f32,
) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let background = low + (high - low) * x as f32 / (extent.x() - 1) as f32;
                let inside = centres.iter().any(|centre| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                        < radius
                });
                let value = if inside {
                    background + contrast
                } else {
                    background
                };
                values[extent.index(x, y, z) as usize] =
                    (value.clamp(0.0, 1.0) * f32::from(HIGH)).round() as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "ramp",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn scattering(extent: Extent, spacing: &[f32; 3], centres: &[[f32; 3]], radius: f32) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let ramp = 0.1 + 0.1 * x as f32 / (extent.x() - 1) as f32;
                let step = 0.3 * smooth((x as f32 - 150.0) / 32.0);
                let background = ramp + step;
                let inside = centres.iter().any(|centre| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                        < radius
                });
                let value = if inside {
                    background + 0.05
                } else {
                    background
                };
                values[extent.index(x, y, z) as usize] =
                    (value.clamp(0.0, 1.0) * f32::from(HIGH)).round() as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "scatter",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn saturated(extent: Extent, spacing: &[f32; 3]) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let plateau =
                    (24..40).contains(&x) && (56..72).contains(&y) && (56..72).contains(&z);
                let dim = ((x as f32 - 140.0).powi(2)
                    + (y as f32 - 64.0).powi(2)
                    + (z as f32 - 64.0).powi(2))
                .sqrt()
                    < 8.0;
                let value = if plateau {
                    f32::from(HIGH)
                } else if dim {
                    0.05 * f32::from(HIGH)
                } else {
                    0.0
                };
                values[extent.index(x, y, z) as usize] = value as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "saturated",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn bridged(extent: Extent, spacing: &[f32; 3]) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let first = ((x as f32 - 70.0).powi(2)
                    + (y as f32 - 48.0).powi(2)
                    + (z as f32 - 48.0).powi(2))
                .sqrt()
                    < 7.0;
                let second = ((x as f32 - 85.0).powi(2)
                    + (y as f32 - 48.0).powi(2)
                    + (z as f32 - 48.0).powi(2))
                .sqrt()
                    < 7.0;
                let noise =
                    (140..142).contains(&x) && (30..32).contains(&y) && (40..42).contains(&z);
                let value = if first || second || noise {
                    f32::from(HIGH)
                } else {
                    0.0
                };
                values[extent.index(x, y, z) as usize] = value as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "bridged",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn mixed(extent: Extent, spacing: &[f32; 3]) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let sphere = ((x as f32 - 40.0).powi(2)
                    + (y as f32 - 60.0).powi(2)
                    + (z as f32 - 60.0).powi(2))
                .sqrt()
                    < 12.0;
                let rod = (100..160).contains(&x) && (58..62).contains(&y) && (58..62).contains(&z);
                let value = if sphere {
                    0.3 * f32::from(HIGH)
                } else if rod {
                    0.9 * f32::from(HIGH)
                } else {
                    0.0
                };
                values[extent.index(x, y, z) as usize] = value as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "mixed",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn shell(extent: Extent, spacing: &[f32; 3], centre: [f32; 3], radius: f32, cavity: f32) -> Volume {
    let count = extent.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let offset = ((x as f32 - centre[0]).powi(2)
                    + (y as f32 - centre[1]).powi(2)
                    + (z as f32 - centre[2]).powi(2))
                .sqrt();
                if offset < radius && offset >= cavity {
                    values[extent.index(x, y, z) as usize] = HIGH;
                }
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(spacing[0], spacing[1], spacing[2]),
        vec![Channel::new(
            "shell",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn smooth(value: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    clamped * clamped * (3.0 - 2.0 * clamped)
}
