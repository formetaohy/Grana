use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use grana_analysis::localization::{LocalizationFilter, LocalizationSpec};
use grana_analysis::{
    Budget, ChannelSet, ColocScan, ColocScanConfig, FilamentScan, Reference, Sources, SpotScanner,
    SurfaceBuilder, TrackLinker, coloc_sampling,
};
use grana_archive::{Channel as WriteChannel, Fill, ImageSpec};
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_image::Source;
use grana_measure::{ColocPair, ColocSeries, ColocSettings, ColocThresholds};
use grana_object::{
    FilamentParams, Link, MaskEdit, MaskMode, ObjectAnchor, Shape, Spot, SpotParams, SurfaceFilter,
    SurfaceParams, SurfaceSet, SurfaceStatistics, TrackDetection, TrackEvents, TrackParams,
    TrackSet,
};
use grana_object::{IntensityTable, SampleStatistics};
use grana_pipeline::Channels;
use grana_scene::{Dataset, Drifts, ObjectFrame, ObjectKind, ObjectParams, Scene};
use grana_store::SpillStore;
use grana_volume::{
    Basis, Calibration, Channel, ChannelInfo, Extent, Intensity, Level, NoData, Pyramid, Region,
    Spacing, Statistics, Volume, VolumeSeries, VoxelData, VoxelFormat, VoxelSemantics,
};
use grana_zarr::write_image;

use crate::support;

const HIGH: u16 = 65_535;

#[test]
fn detects_and_localises_bright_spots() {
    let extent = Extent::new(96, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let data = spot_volume(extent, spacing);
    let level = Level::new(extent, spacing);
    let scanner = SpotScanner::new(
        0,
        level,
        data.range(),
        SpotParams::new(0.5, 0.2),
        Region::none(),
        ChannelSet::new(0, &[None]),
        Budget::new(2),
    );
    let voxels = AtomicU64::new(0);
    let read = |channel: usize, ranges: &[std::ops::Range<u32>; 3]| {
        assert_eq!(channel, 0, "the spot scan reads its detection channel");
        let region = data.region(
            level.extent(),
            ranges[0].clone(),
            ranges[1].clone(),
            ranges[2].clone(),
        );
        voxels.fetch_add(region.voxel_count() as u64, Ordering::Relaxed);
        Ok(region)
    };
    let set = scanner
        .frame(&AtomicBool::new(false), None, &read, &mut |_, _| {})
        .expect("the spot frame is detected")
        .expect("the spot frame is not cancelled");
    let spots = set.spots();
    assert_eq!(spots.len(), 2, "each bright blob is one spot");
    let volume = extent.voxel_count();
    let read = voxels.load(Ordering::Relaxed);
    assert!(
        read < 3 * volume,
        "the detection channel is read once into the detection image plus once per block, {read} voxels for a {volume} voxel region"
    );
    let world = level.world_extent();
    for centre in [[24.0f32, 32.0, 16.0], [66.0, 32.0, 16.0]] {
        let expected = [0, 1, 2].map(|axis| {
            let spacing = spacing_axis(spacing, axis);
            (centre[axis] + 0.5) * spacing - 0.5 * world[axis]
        });
        let nearest = spots
            .iter()
            .map(|spot| {
                let center = spot.center();
                let distance = (0..3)
                    .map(|axis| (center[axis] - expected[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                (distance, spot.quality())
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .expect("a spot was detected");
        assert!(
            nearest.0 < 0.4,
            "the ball at {expected:?} was localised {:.3} away",
            nearest.0
        );
        assert!(
            nearest.1 > 0.2,
            "the ball at {expected:?} has a weak contrast {}",
            nearest.1
        );
    }
}

fn spot_volume(extent: Extent, spacing: Spacing) -> VoxelData {
    let sigma = 0.5 / 2.35482;
    let mut values = vec![200.0f32; extent.voxel_count() as usize];
    for centre in [[24.0f32, 32.0, 16.0], [66.0, 32.0, 16.0]] {
        for z in 0..extent.z() {
            for y in 0..extent.y() {
                for x in 0..extent.x() {
                    let offset = [
                        (x as f32 - centre[0]) * spacing.x(),
                        (y as f32 - centre[1]) * spacing.y(),
                        (z as f32 - centre[2]) * spacing.z(),
                    ];
                    let squared = offset.iter().map(|value| value * value).sum::<f32>();
                    let value = 12_000.0 * (-squared / (2.0 * sigma * sigma)).exp();
                    values[extent.index(x, y, z) as usize] += value;
                }
            }
        }
    }
    VoxelData::U16(
        values
            .into_iter()
            .map(|value| value.min(f32::from(u16::MAX)) as u16)
            .collect(),
    )
}
fn spacing_axis(spacing: Spacing, axis: usize) -> f32 {
    match axis {
        0 => spacing.x(),
        1 => spacing.y(),
        _ => spacing.z(),
    }
}

#[test]
fn extracts_measured_surfaces_from_two_balls() {
    let extent = Extent::new(80, 64, 48);
    let spacing = Spacing::new(0.2, 0.2, 0.2);
    let radius = 8.0;
    let volume = support::spheres(
        extent,
        spacing,
        &[([24.0, 32.0, 24.0], radius), ([56.0, 32.0, 24.0], radius)],
        HIGH,
    );
    let set = extract_surfaces(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
    );
    assert_eq!(set.object_count(), 2, "each ball is one object");
    assert!(
        set.geometry().is_ok_and(|mesh| mesh.triangle_count() > 0),
        "the surface carries triangles"
    );
    let expected = 4.0 / 3.0 * PI * (f64::from(radius) * 0.2).powi(3);
    let level = Level::new(extent, spacing);
    let world = level.world_extent();
    let expected_centres = [[24.0f32, 32.0, 24.0], [56.0, 32.0, 24.0]]
        .map(|centre| [0, 1, 2].map(|axis| (centre[axis] + 0.5) * 0.2 - 0.5 * world[axis]));
    for object in set.objects() {
        let relative = (object.volume() - expected).abs() / expected;
        assert!(
            relative < 0.2,
            "a ball volume {} is off from {expected}",
            object.volume()
        );

        assert!(object.area() > 0.0, "the object carries an area");
        assert!(
            (0.6..=1.0).contains(&object.sphericity()),
            "a ball is not spherical: {}",
            object.sphericity()
        );
        let shape = object.shape().expect("a ball carries a shape");
        let radius_um = f64::from(radius) * 0.2;
        for axis in shape.axes() {
            let relative = (axis - radius_um).abs() / radius_um;
            assert!(
                relative < 0.1,
                "a ball semi axis {axis} is off from {radius_um}"
            );
        }
        let diameter = shape.diameter();
        let expected_diameter = 2.0 * radius_um;
        assert!(
            (diameter - expected_diameter).abs() / expected_diameter < 0.1,
            "a ball diameter {diameter} is off from {expected_diameter}"
        );
        let hull = shape.hull_volume();
        assert!(
            (hull - expected).abs() / expected < 0.15,
            "a ball hull holds {hull} of the expected {expected}"
        );
        let centre = object.centroid();
        let placed = expected_centres.iter().any(|candidate| {
            (0..3)
                .map(|axis| f64::from(centre[axis] - candidate[axis]).powi(2))
                .sum::<f64>()
                < 0.25
        });
        assert!(placed, "the object centroid {centre:?} is misplaced");
    }

    let fill = HIGH - 1;
    let mut holed = match volume.channels()[0].data() {
        VoxelData::U16(values) => values.clone(),
        other => panic!("a ball volume is sixteen bit, found {:?}", other.format()),
    };
    for z in 0..8 {
        for y in 0..8 {
            for x in 0..8 {
                holed[extent.index(x, y, z) as usize] = fill;
            }
        }
    }
    let holey = Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "signal",
            [0.3, 0.6, 1.0],
            VoxelData::U16(holed),
        )],
    );
    let filled = extract_surfaces_at(
        &holey,
        level,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        None,
        Some(NoData::U16(fill)),
    );
    assert_eq!(
        filled.object_count(),
        2,
        "a declared missing body is no surface to detect"
    );
    for (plain, holed) in set.objects().iter().zip(filled.objects()) {
        let relative = (plain.volume() - holed.volume()).abs() / plain.volume();
        assert!(
            relative < 0.02,
            "a missing body nearby moved a ball volume by {relative}"
        );
    }

    let level = Level::new(extent, spacing);
    let centre = level.voxel_center([24.0, 32.0, 24.0]);
    let roi = grana_object::Mask::boxed(
        Region::new(
            glam::Vec3::from(centre),
            [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z],
            glam::Vec3::splat(1.7),
        ),
        level,
    );
    let masked = extract_surfaces_at(
        &volume,
        level,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Some(&roi),
        None,
    );
    assert_eq!(
        masked.object_count(),
        1,
        "an analysis mask that covers one ball keeps that surface only"
    );

    let tiny = grana_object::Mask::boxed(
        Region::new(
            glam::Vec3::from(centre),
            [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z],
            glam::Vec3::splat(0.3),
        ),
        level,
    );
    let small = extract_surfaces_at(
        &volume,
        level,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
        Some(&tiny),
        None,
    );
    assert_eq!(
        small.object_count(),
        1,
        "a region too small to estimate local background still detects its bright voxels"
    );
    let clipped = small.object(0).expect("the clipped surface exists");
    assert!(
        clipped.sphericity() < 0.99,
        "a clipped fragment of a ball reads its own sphericity {}, not a saturated 1",
        clipped.sphericity()
    );
    let geometry = small.geometry().expect("the clipped geometry loads");
    let group = &geometry.groups()[0];
    let positions = (0..group.vertex_count() as usize)
        .map(|vertex| geometry.position(0, vertex))
        .collect::<Vec<_>>();
    let triangles = (0..group.index_count() as usize / 3)
        .map(|triangle| geometry.triangle_vertices(0, triangle))
        .collect::<Vec<_>>();
    let enclosed = grana_geometry::enclosed_volume(&positions, &triangles);
    assert!(
        (clipped.volume() - enclosed).abs() <= 1.0e-3 * enclosed,
        "the reported volume {} belongs to the closed mesh that encloses {enclosed} and carries the area {}",
        clipped.volume(),
        clipped.area()
    );
}

#[test]
fn floods_a_wand_past_the_high_side_of_its_block() {
    let extent = Extent::new(512, 32, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let level = Level::new(extent, spacing);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for x in 100..300 {
        for y in 16..18 {
            for z in 16..18 {
                values[extent.index(x, y, z) as usize] = HIGH;
            }
        }
    }
    let image = Arc::new(
        grana_image::Image::open(&Source::memory(VolumeSeries::new(
            vec![Volume::new(
                extent,
                spacing,
                vec![Channel::new(
                    "signal",
                    [0.3, 0.6, 1.0],
                    VoxelData::U16(values),
                )],
            )],
            1.0,
        )))
        .expect("the rod opens"),
    );
    let mut scene = Scene::new(Dataset::synthetic("rod", image.pyramid().clone()));
    let set = scene.add_object(ObjectKind::Surfaces, 0);
    let low = [100u32, 16, 16];
    let high = [102u32, 18, 18];
    let centroid = level.voxel_center([101.0, 17.0, 17.0]);
    let bounds = Bounds::new(
        level.corner(low.map(|value| value as f32)),
        level.corner(high.map(|value| value as f32)),
    );
    let statistics =
        SurfaceStatistics::new(8, 8.0 * level.voxel_volume(), 0.24, centroid, bounds, None);
    scene.install(
        set,
        0,
        ObjectFrame::Surfaces(SurfaceSet::new(
            0,
            cube(level, low, high),
            vec![statistics],
            IntensityTable::new(
                1,
                vec![Some(SampleStatistics::new(
                    5000.0,
                    0.0,
                    f32::from(HIGH),
                    1.0,
                    40_000.0,
                ))],
            ),
        )),
    );
    let object = scene.objects.get(set).expect("the base set installs");
    let source = object.source();
    let channels = Channels::render(Arc::clone(&image), &scene);
    let stroke = MaskEdit::wand(
        0,
        ObjectAnchor::new(centroid, bounds),
        MaskMode::Add,
        0.1,
        centroid,
    )
    .expect("the wand stroke builds");
    let drifts = Drifts::new();
    let edited = grana_pipeline::run::mask(
        &channels,
        &source,
        object.voxel(),
        &grana_pipeline::run::MaskRequest {
            channel: 0,
            strokes: &[stroke],
            drifts: &drifts,
        },
    )
    .expect("the wand closes on the dim side of its block");
    assert_eq!(edited.len(), 1, "the wand edits one frame");
    let Some(frame) = edited[0].1.as_deref() else {
        panic!("the wand keeps the object");
    };
    let ObjectFrame::Surfaces(flooded) = frame else {
        panic!("the wand keeps a surface");
    };
    assert_eq!(
        flooded.objects()[0].voxel_count(),
        800,
        "the wand keeps every voxel of the rod it flooded"
    );
}

fn cube(level: Level, low: [u32; 3], high: [u32; 3]) -> Mesh {
    let point = |bits: u32| {
        let corner = [0, 1, 2].map(|axis| {
            if (bits >> axis) & 1 == 0 {
                low[axis] as f32
            } else {
                high[axis] as f32
            }
        });
        level.corner(corner)
    };
    let vertices = (0..8u32)
        .map(|bits| MeshVertex::new(point(bits), [0.0, 0.0, 1.0], 0))
        .collect();
    let indices = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6,
        1, 3, 5, 3, 7, 5,
    ];
    Mesh::new(vertices, indices)
}

fn extract_surfaces(volume: &grana_volume::Volume, params: SurfaceParams) -> SurfaceSet {
    extract_surfaces_at(
        volume,
        Level::new(volume.extent(), volume.spacing()),
        params,
        None,
        None,
    )
}

fn extract_surfaces_at(
    volume: &grana_volume::Volume,
    level: Level,
    params: SurfaceParams,
    roi: Option<&grana_object::Mask>,
    no_data: Option<NoData>,
) -> SurfaceSet {
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();
    let missing = vec![no_data; channels];
    let builder = SurfaceBuilder::new(
        0,
        level,
        params,
        ChannelSet::new(0, &missing),
        Region::none(),
        intensity,
    );
    let cancel = AtomicBool::new(false);
    let scan = builder
        .scan(
            Budget::new(4),
            &cancel,
            roi,
            &mut |_done, _total| {},
            Sources::new(
                &|ranges| {
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
                &SpillStore::memory(),
            ),
        )
        .expect("a surface frame is detected");
    match scan {
        grana_analysis::SurfaceScan::Set(set) => set,
        grana_analysis::SurfaceScan::Empty => SurfaceSet::empty(0, channels),
        grana_analysis::SurfaceScan::Cancelled => {
            panic!("a scan without a cancelled flag completes")
        }
    }
}

#[test]
fn relates_objects_by_surface_centre_and_neighbours() {
    let pair = relation_surfaces(&[([20.0, 32.0, 24.0], 8.0), ([52.0, 32.0, 24.0], 8.0)]);
    assert_eq!(pair.object_count(), 2, "each ball is one object");
    let mesh = pair.geometry().expect("the pair carries geometry");
    let neighbours =
        Reference::new(&Shape::mesh(mesh.clone())).relate(&Shape::mesh(mesh), 0.0, true);
    assert_eq!(neighbours.objects(), 2);
    assert_eq!(
        neighbours.pairs().len(),
        2,
        "each neighbour names its partner as a pair"
    );
    for object in 0..2 {
        assert!(
            (neighbours.center(object) - 6.4).abs() < 0.3,
            "the neighbours sit {} apart",
            neighbours.center(object)
        );
        assert!(
            (neighbours.distance(object) - 3.2).abs() < 0.3,
            "the surfaces sit {} apart",
            neighbours.distance(object)
        );
        assert_eq!(
            neighbours.overlapping(object),
            0,
            "the balls do not overlap"
        );
        assert_eq!(neighbours.partner(object), if object == 0 { 2 } else { 1 });
        let pair = neighbours
            .pairs()
            .iter()
            .find(|pair| pair.source() as usize == object)
            .expect("the object keeps one pair");
        assert_eq!(
            pair.target(),
            u32::from(object == 0),
            "the pair points at the neighbour"
        );
        assert!(
            (pair.distance() - 3.2).abs() < 0.3,
            "the pair sits {} apart",
            pair.distance()
        );
        assert!(
            (pair.center() - 6.4).abs() < 0.3,
            "the paired centres sit {} apart",
            pair.center()
        );
        assert_eq!(pair.contact(), 0.0, "the pair shares no surface");
        assert_eq!(pair.inside(), 0.0, "the pair holds no containment");
    }

    let first = relation_surfaces(&[([24.0, 32.0, 24.0], 8.0)]);
    let second = relation_surfaces(&[([26.0, 32.0, 24.0], 8.0)]);
    let source = Shape::mesh(first.geometry().expect("the source carries geometry"));
    let overlap = Reference::new(&Shape::mesh(
        second.geometry().expect("the target carries geometry"),
    ))
    .relate(&source, 0.0, false);
    assert_eq!(
        overlap.overlapping(0),
        1,
        "the source lies inside its partner"
    );
    assert_eq!(overlap.partner(0), 1);
    assert!(
        overlap.inside(0) > 0.3,
        "only {} of the source lies inside",
        overlap.inside(0)
    );
    assert!(
        overlap.center(0) < 0.8,
        "the overlapped centres sit {} apart",
        overlap.center(0)
    );
    assert_eq!(overlap.distance(0), 0.0, "an overlapping surface touches");
    let pair = overlap.pairs().first().expect("the overlap keeps a pair");
    assert_eq!(pair.target(), 0);
    assert!(
        (pair.inside() - overlap.inside(0)).abs() < 1.0e-6,
        "the pair carries the containment of its object"
    );
    assert!(
        pair.inside() > 0.3,
        "only {} of the pair lies inside",
        pair.inside()
    );
    assert_eq!(pair.distance(), 0.0, "the overlapping pair touches");
    assert!(
        pair.center() < 0.8,
        "the paired centres sit {} apart",
        pair.center()
    );

    let far = relation_surfaces(&[([52.0, 32.0, 24.0], 8.0)]);
    let apart = Reference::new(&Shape::mesh(
        far.geometry().expect("the distant ball carries geometry"),
    ))
    .relate(&source, 0.0, false);
    assert!(
        (apart.center(0) - 5.6).abs() < 0.3,
        "the centres sit {} apart",
        apart.center(0)
    );
    assert!(
        (apart.distance(0) - 2.4).abs() < 0.3,
        "the surfaces sit {} apart",
        apart.distance(0)
    );
    assert_eq!(apart.overlapping(0), 0);
    let pair = apart.pairs().first().expect("the distance keeps a pair");
    assert!(
        (pair.distance() - 2.4).abs() < 0.3,
        "the pair sits {} apart",
        pair.distance()
    );

    let surface = Reference::new(&Shape::mesh(
        first.geometry().expect("the ball carries geometry"),
    ));
    let measure = |reach: f32, gate: f32| {
        let frame = surface.relate(&Shape::spots(approaching(reach)), gate, false);
        (frame.distance(0), frame.contact(0))
    };
    let (reach, area) = measure(0.3, 0.0);
    assert!(
        (reach - 0.3).abs() < 0.15,
        "the bead sits {reach:.2} 碌m from the surface"
    );
    let expected = std::f32::consts::TAU * BEAD * (BEAD - reach);
    assert!(
        (area - expected).abs() < 0.6,
        "the cut bead shares {area:.2} 碌m铏?against {expected:.2}"
    );
    let (_, gated) = measure(0.3, 0.5);
    assert!(
        gated > area + 1.0,
        "a wider gate shares {gated:.2} 碌m铏?against {area:.2}"
    );
    let (_, apart) = measure(3.0, 0.0);
    assert_eq!(apart, 0.0, "a bead beyond its reach shares no face");
}

const BALL: f32 = 8.0;
const BEAD: f32 = 1.2;
const BALL_SPACING: f32 = 0.2;

fn approach_grid() -> Level {
    Level::new(
        Extent::new(64, 64, 48),
        Spacing::new(BALL_SPACING, BALL_SPACING, BALL_SPACING),
    )
}

fn approaching(reach: f32) -> Arc<[Spot]> {
    let center = approach_grid().voxel_center([24.0, 32.0, 24.0]);
    let spot = Spot::new(
        [
            center[0] + BALL * BALL_SPACING + reach,
            center[1],
            center[2],
        ],
        BEAD,
        1.0,
    );
    Arc::from(vec![spot])
}

fn relation_surfaces(balls: &[([f32; 3], f32)]) -> SurfaceSet {
    let volume = support::spheres(
        Extent::new(64, 64, 48),
        approach_grid().spacing(),
        balls,
        HIGH,
    );
    extract_surfaces(
        &volume,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
    )
}

#[test]
fn traces_a_dim_filament_on_a_coarse_grid() {
    let extent = Extent::new(40, 40, 30);
    let spacing = Spacing::new(0.3, 0.3, 0.1);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    let segment = |x: f32, y: f32, from: (f32, f32), to: (f32, f32)| {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let (px, py) = (x - from.0, y - from.1);
        let along = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - along * dx).powi(2) + (py - along * dy).powi(2)).sqrt()
    };
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let across = segment(x as f32, y as f32, (4.0, 20.0), (36.0, 20.0)).min(segment(
                    x as f32,
                    y as f32,
                    (20.0, 20.0),
                    (20.0, 36.0),
                ));
                let offset = across.hypot(z as f32 - 15.0);
                let width = 0.35 + 1.35 * x as f32 / extent.x() as f32;
                let intensity = 300.0 + 4_000.0 * (-0.5 * (offset / width).powi(2)).exp();
                values[extent.index(x, y, z) as usize] = intensity as u16;
            }
        }
    }
    let data = VoxelData::U16(values);
    let level = Level::new(extent, spacing);
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let mut scan = FilamentScan::new(
        0,
        level,
        FilamentParams::new(8_000.0 / f32::from(HIGH), 2.5),
        ChannelSet::new(0, &[None]),
        Region::none(),
        intensity,
        Budget::new(1),
    );
    let set = scan
        .frame(&data, None, &SpillStore::memory(), |_| Ok(data.clone()))
        .expect("the filament scan traces the frame");
    assert_eq!(set.object_count(), 1, "the dim filament survives");
    let filament = set.object(0).expect("the filament exists");
    let length = filament.length();
    let expected = f64::from(extent.x()) * f64::from(spacing.x());
    assert!(
        length > 0.5 * expected,
        "the traced length {length:.2} keeps most of {expected:.2}"
    );
    let cylinder = std::f64::consts::PI / 4.0 * filament.diameter().powi(2) * length;
    assert!(
        filament.volume() > 0.95 * cylinder && filament.volume() < 1.2 * cylinder,
        "the volume {} follows the tapered tube that keeps {cylinder}, not a mean radius cylinder",
        filament.volume()
    );
}

#[test]
fn pages_localization_frames_beyond_the_resident_budget() {
    const FRAMES: u32 = 1200;
    const PER_FRAME: usize = 1000;
    let level = Level::new(Extent::new(32, 32, 2), Spacing::new(0.1, 0.1, 0.1));
    let pyramid = Pyramid::new(
        vec![level],
        1,
        vec![ChannelInfo::new(
            "GFP",
            [0.0, 1.0, 0.0],
            Statistics::declared(Intensity::new(0.0, 255.0), VoxelSemantics::Scalar),
            VoxelFormat::U16,
        )],
        FRAMES,
        1.0,
        Calibration::Recorded,
    );
    let mut scene = Scene::new(Dataset::synthetic("paged", pyramid));
    let set = scene.add_object(ObjectKind::Spots, 0);
    let statistics = SampleStatistics::new(500.0, 400.0, 600.0, 20.0, 500.0);
    let center =
        |time: u32, index: usize| [0.01 * index as f32, 0.02 * time as f32, 0.5 * index as f32];
    for time in 0..FRAMES {
        let spots = (0..PER_FRAME)
            .map(|index| Spot::new(center(time, index), 0.05, 1.0))
            .collect::<Vec<_>>();
        let intensities = IntensityTable::new(1, vec![Some(statistics); PER_FRAME]);
        scene.install(
            set,
            time,
            ObjectFrame::from_spots(grana_object::SpotSet::new(0, spots, intensities)),
        );
    }
    let object = scene.objects.get(set).expect("the spots stay");
    assert!(
        object.paged(),
        "a {FRAMES} frame localization set pages its frames"
    );
    assert!(
        object.paged_bytes() <= 64 << 20,
        "the resident localization pages stay inside the budget"
    );
    assert_eq!(object.object_count(0), PER_FRAME);
    for time in [0, 1, 599, FRAMES - 2, FRAMES - 1] {
        let frame = object.frame_shared(time).expect("the paged frame reloads");
        let spots = frame.spots().expect("the paged frame holds spots");
        assert_eq!(
            spots.len(),
            PER_FRAME,
            "the paged frame keeps its localizations"
        );
        let index = time as usize % PER_FRAME;
        assert_eq!(spots.spots()[index].center(), center(time, index));
    }
    let total = object
        .frames()
        .map(|(_, frame)| frame.object_count())
        .sum::<usize>();
    assert_eq!(total, FRAMES as usize * PER_FRAME);
    let window = grana_pipeline::run::LocalizationWindow::of(object, [0, 4]);
    let report = grana_analysis::localization::localization_report(
        &window,
        LocalizationSpec::new([0, 4], 100.0, 0.0),
        0.1,
        true,
        None,
        &mut |_, _| true,
    )
    .expect("the paged window renders")
    .expect("the paged window completes");
    assert_eq!(
        report.localizations(),
        5 * PER_FRAME as u64,
        "the paged window reads every localization of its range"
    );
    assert!(
        report
            .stack()
            .layer(0)
            .is_some_and(|image| image.pixels().iter().any(|sample| *sample > 0)),
        "the paged window paints its localizations"
    );
    let before = scene
        .objects
        .get(set)
        .expect("the spots stay")
        .paged_bytes();
    scene.set_mask(Some(set));
    let roi = scene
        .mask_frames()
        .expect("the analysis mask binds its object set")
        .expect("the analysis mask names its object set");
    assert_eq!(
        roi.frames(),
        FRAMES as usize,
        "the analysis mask spans every frame of its object set"
    );
    assert_eq!(
        scene
            .objects
            .get(set)
            .expect("the spots stay")
            .paged_bytes(),
        before,
        "binding the analysis mask leaves the resident pages untouched"
    );
    let mask = roi
        .mask(scene.dataset.pyramid().level(0), 7)
        .expect("the analysis mask rasterizes one frame");
    assert!(
        !mask.is_empty(),
        "the analysis mask covers the localizations it keeps"
    );
}

#[test]
fn estimates_localization_drift_from_density() {
    let frames = 600usize;
    let rate = 0.002f32;
    let axial_rate = 0.001f32;
    let reference = 300u32;
    let mut source = 0x1234_5678u32;
    let mut value = || {
        source = source.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (source >> 8) as f32 / 16_777_216.0
    };
    let base = (0..300)
        .map(|_| [8.0 * value(), 8.0 * value(), 0.8 * value() - 0.4])
        .collect::<Vec<_>>();
    let series = (0..frames)
        .map(|time| {
            let shift = rate * time as f32;
            let axial = axial_rate * time as f32;
            base.iter()
                .map(|point| {
                    grana_analysis::LocalizationPoint::new(
                        [point[0] + shift, point[1], point[2] + axial],
                        [0.012; 3],
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let found = grana_analysis::localization_drift(&series, reference, &mut |_, _| true)
        .expect("the drift estimates")
        .expect("the drift finishes");
    let offsets = found.offsets();
    assert_eq!(offsets.len(), frames);
    for (time, offset) in offsets.iter().enumerate() {
        let expected = rate * (time as f32 - reference as f32);
        let axial = axial_rate * (time as f32 - reference as f32);
        assert!(
            (offset[0] - expected).abs() < 0.15,
            "the frame {time} drifts {offset:?} against {expected}"
        );
        assert!(offset[1].abs() < 0.15, "the frame {time} stays on y");
        assert!(
            (offset[2] - axial).abs() < 0.1,
            "the frame {time} rises {offset:?} against {axial}"
        );
    }
}

#[test]
fn registers_channels_from_fiducial_beads() {
    let mut reference = Vec::new();
    for row in 0..6 {
        for column in 0..6 {
            reference.push([column as f32 * 12.0, row as f32 * 12.0, 0.0]);
        }
    }
    let moving = reference
        .iter()
        .map(|point| {
            [
                point[0] + 0.8 - 0.004 * point[0],
                point[1] + 0.3 + 0.002 * point[1],
                point[2],
            ]
        })
        .collect::<Vec<_>>();
    let fiducial = grana_analysis::fiducial_registration(&reference, &moving, 0.1)
        .expect("the fiducials register the channels");
    assert_eq!(fiducial.pairs(), reference.len());
    assert!(
        fiducial.residual() < 0.05,
        "the fiducial fit leaves {:.3} 绾板",
        fiducial.residual()
    );
    for (expected, moving) in reference.iter().zip(&moving) {
        let offset = fiducial.warp().sample(*moving);
        let corrected = [0, 1, 2].map(|axis| moving[axis] - offset[axis]);
        for axis in 0..2 {
            assert!(
                (corrected[axis] - expected[axis]).abs() < 0.05,
                "the bead at {moving:?} lands {corrected:?} from {expected:?}"
            );
        }
    }
}

fn moved(source: &[f32], dims: [usize; 3], offset: [f32; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; source.len()];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let at = [
                    x as f32 - offset[0],
                    y as f32 - offset[1],
                    z as f32 - offset[2],
                ];
                values[x + dims[0] * (y + dims[1] * z)] = sample(source, dims, at);
            }
        }
    }
    values
}

fn sample(source: &[f32], dims: [usize; 3], at: [f32; 3]) -> f32 {
    let low = at.map(|value| value.floor() as i64);
    let fraction = [0, 1, 2].map(|axis| at[axis] - low[axis] as f32);
    let mut sum = 0.0;
    for corner in 0..8 {
        let offset = [0, 1, 2].map(|axis| (corner >> axis) & 1);
        let weight = (0..3)
            .map(|axis| {
                if offset[axis] == 1 {
                    fraction[axis]
                } else {
                    1.0 - fraction[axis]
                }
            })
            .product::<f32>();
        let at = [0, 1, 2].map(|axis| low[axis] + offset[axis]);
        if at
            .iter()
            .enumerate()
            .any(|(axis, value)| *value < 0 || *value >= dims[axis] as i64)
        {
            continue;
        }
        sum +=
            weight * source[at[0] as usize + dims[0] * (at[1] as usize + dims[1] * at[2] as usize)];
    }
    sum
}

#[test]
fn measures_correlated_channels() {
    let extent = Extent::new(48, 32, 8);
    let first = ramp(extent);
    let range = Intensity::new(0.0, f32::from(HIGH));
    let series = coloc_series(extent, &first, &first, [range; 2]);
    let statistics = series.statistics(ColocThresholds::new(0.5, 0.5));
    assert_eq!(statistics.voxels(), extent.voxel_count());
    assert!(
        statistics
            .pearson()
            .is_some_and(|value| (value - 1.0).abs() < 1.0e-6)
    );
    assert!(
        statistics
            .overlap()
            .is_some_and(|value| (value - 1.0).abs() < 1.0e-6)
    );
    let joint = series.joint();
    assert_eq!(joint.total().iter().sum::<u64>(), extent.voxel_count());
    assert_eq!(
        joint
            .frame(0)
            .map(|cells| cells.iter().map(|cell| cell.count()).sum::<u64>()),
        Some(extent.voxel_count())
    );
    assert_eq!(series.level(), 0);
    assert_eq!(series.coverage(), 1.0);
    let significance = series
        .significance()
        .expect("the series carries a permutation test");
    assert_eq!(significance.randomizations(), 64);
    assert!(
        significance.p() < 0.05,
        "a perfectly correlated pair tests significant"
    );
    let zeros = VoxelData::U16(vec![0; extent.voxel_count() as usize]);
    let unthresholded = coloc_series(extent, &first, &zeros, [range; 2])
        .statistics(ColocThresholds::new(0.0, 0.0))
        .manders()[0]
        .expect("the first channel carries mass");
    assert!(
        (unthresholded - 1.0).abs() < 1.0e-9,
        "a zero threshold keeps the first channel mass {unthresholded}"
    );
    let (low, high) = halves(extent);
    let weighted = coloc_series(
        extent,
        &low,
        &high,
        [Intensity::new(10.0, 20.0), Intensity::new(1.0, 3.0)],
    )
    .statistics(ColocThresholds::new(0.5, 0.5));
    let overlap = weighted.overlap().expect("the channels share mass");
    assert!(
        (overlap - 280.0 / 80_000f64.sqrt()).abs() < 1.0e-6,
        "the overlap coefficient weights raw intensities {overlap}"
    );
    let manders = weighted.manders()[0].expect("the first channel carries mass");
    assert!(
        (manders - 2.0 / 3.0).abs() < 1.0e-6,
        "manders weighs the raw intensity of the first channel {manders}"
    );
}

fn halves(extent: Extent) -> (VoxelData, VoxelData) {
    let half = extent.x() / 2;
    let mut low = Vec::with_capacity(extent.voxel_count() as usize);
    let mut high = Vec::with_capacity(extent.voxel_count() as usize);
    for _z in 0..extent.z() {
        for _y in 0..extent.y() {
            for x in 0..extent.x() {
                let left = x < half;
                low.push(if left { 10 } else { 20 });
                high.push(if left { 1 } else { 3 });
            }
        }
    }
    (VoxelData::U16(low), VoxelData::U16(high))
}

fn ramp(extent: Extent) -> VoxelData {
    let mut values = Vec::with_capacity(extent.voxel_count() as usize);
    for _z in 0..extent.z() {
        for _y in 0..extent.y() {
            for x in 0..extent.x() {
                values.push((x as f32 / (extent.x() - 1) as f32 * f32::from(HIGH)) as u16);
            }
        }
    }
    VoxelData::U16(values)
}

fn coloc_series(
    extent: Extent,
    first: &VoxelData,
    second: &VoxelData,
    intensities: [Intensity; 2],
) -> ColocSeries {
    let spacing = Spacing::new(0.1, 0.1, 0.3);
    let level = Level::new(extent, spacing);
    let channels = intensities
        .iter()
        .enumerate()
        .map(|(channel, intensity)| {
            ChannelInfo::new(
                format!("Channel {channel}"),
                [1.0, 1.0, 1.0],
                Statistics::declared(*intensity, VoxelSemantics::Scalar),
                VoxelFormat::U16,
            )
        })
        .collect();
    let pyramid = Pyramid::new(vec![level], 1, channels, 1, 1.0, Calibration::Recorded);
    let settings = ColocSettings::new(64, 3);
    let sampling = coloc_sampling(
        &pyramid,
        Region::none(),
        settings.precision(),
        settings.block(),
    );
    let info = pyramid.level(sampling.level());
    let pair = ColocPair::new(0, 1);
    let mut scan = ColocScan::new(
        info,
        sampling,
        ColocScanConfig::new(pair, 2, intensities, [None, None], None),
        Region::none(),
    );
    let cancel = AtomicBool::new(false);
    let mut quiet = |_: usize, _: usize| {};
    let read = |ranges: &[std::ops::Range<u32>; 3], channel: usize| {
        let data = if channel == 0 { first } else { second };
        data.region(
            extent,
            ranges[0].clone(),
            ranges[1].clone(),
            ranges[2].clone(),
        )
    };
    let scanned = scan
        .scan(
            2,
            &cancel,
            &|_time, channel, ranges| Ok(read(ranges, channel)),
            &|_time| Ok(None),
            &mut quiet,
        )
        .expect("the scan completes");
    assert!(scanned, "the scan is not cancelled");
    let significance = scan.test(settings.permutations(), 2, &cancel, &mut quiet);
    scan.finish(significance, settings.permutations())
}

#[test]
fn links_divisions_and_merges_jointly() {
    let set = tracked(
        &[
            (&[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]][..], 1.0),
            (&[[0.5, 0.0, 0.0]][..], 1.0),
            (&[[1.0, 0.0, 0.0], [5.2, 0.0, 0.0]][..], 1.0),
        ],
        TrackParams::new(1.0, 1, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(set.len(), 3);
    let moving = set.track_at(2, 0).expect("the moving object is tracked");
    assert_eq!(
        set.track(moving).expect("the track exists").nodes().len(),
        3
    );
    assert_eq!(set.links().len(), 1);
    assert_eq!(set.links()[0].kind(), Link::Gap);
    assert_eq!(set.parent(2), Some(1));
    assert_eq!(
        set.statistics(2)
            .expect("the gap track holds one node")
            .frames(),
        1
    );

    let apart = tracked(
        &[(&[[0.0, 0.0, 0.0]][..], 1.0), (&[[0.8, 0.0, 0.0]][..], 1.0)],
        TrackParams::new(1.0, 0, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(
        apart.len(),
        2,
        "a match too far inside the gate is a death and a birth, not a link"
    );
    assert!(
        apart.links().is_empty(),
        "the distant detection starts over"
    );

    let carried = tracked(
        &[(&[[0.0, 0.0, 0.0]][..], 3.0), (&[[2.0, 0.0, 0.0]][..], 3.0)],
        TrackParams::new(1.0, 0, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(
        carried.len(),
        1,
        "a large object travels as far as its own radius"
    );

    let divided = tracked(
        &[
            (&[[0.0, 0.0, 0.0]][..], 0.5),
            (&[[-0.3, 0.0, 0.0], [0.3, 0.0, 0.0]][..], 0.5),
            (&[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]][..], 0.5),
        ],
        TrackParams::new(0.5, 0, events(true, 1.5, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(
        divided.len(),
        3,
        "a mother that ends between two daughters divides into them"
    );
    assert_eq!(divided.divisions(), 1);
    assert_eq!(divided.children(0).count(), 2);

    let claimed = tracked(
        &[
            (
                &[[0.0, 0.0, 0.0], [1.6, -0.4, 0.0], [-1.6, -0.4, 0.0]][..],
                0.5,
            ),
            (
                &[[0.0, 0.0, 0.0], [1.2, -0.2, 0.0], [-1.2, -0.2, 0.0]][..],
                0.5,
            ),
            (&[[0.5, 0.0, 0.0], [-0.5, 0.0, 0.0]][..], 0.5),
        ],
        TrackParams::new(0.5, 0, events(true, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(
        claimed.divisions(),
        1,
        "a division still explains daughters that weaker links already held"
    );
    let daughter = claimed
        .track_at(2, 0)
        .expect("the first daughter is tracked");
    assert_eq!(
        claimed
            .parents(daughter)
            .map(|link| link.kind())
            .collect::<Vec<_>>(),
        vec![Link::Division]
    );

    let frames = || {
        [
            (&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]][..], 1.2),
            (&[[1.0, 0.0, 0.0]][..], 1.5),
            (&[[1.2, 0.0, 0.0]][..], 1.5),
        ]
    };
    let merged = tracked(
        &frames(),
        TrackParams::new(2.0, 0, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(merged.merges(), 1);
    let joined = merged.track_at(2, 0).expect("the merged object is tracked");
    assert_eq!(merged.parents(joined).count(), 2);
    assert!(
        merged
            .parents(joined)
            .all(|link| link.kind() == Link::Merge)
    );
    assert_eq!(
        merged
            .track(joined)
            .expect("the merged track exists")
            .nodes()
            .len(),
        2
    );
    let overlapping = tracked(
        &frames(),
        TrackParams::new(2.0, 0, events(false, 2.0, 2.0, 0.35)),
        1.0,
    );
    assert_eq!(
        overlapping.merges(),
        0,
        "a merge needs the sources to overlap as much as the caller asks"
    );
    let gated = tracked(
        &[
            (&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]][..], 1.2),
            (&[[3.9, 0.0, 0.0]][..], 1.5),
        ],
        TrackParams::new(2.0, 0, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(gated.merges(), 0);
}

fn tracked(frames: &[(&[[f32; 3]], f32)], params: TrackParams, frame_interval: f32) -> TrackSet {
    let frames = frames
        .iter()
        .map(|(centroids, radius)| detections(centroids, *radius))
        .collect::<Vec<_>>();
    let slices = frames.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let cancel = AtomicBool::new(false);
    let mut quiet = |_: usize, _: usize| {};
    TrackLinker::new(params)
        .link(&slices, frame_interval, &cancel, &mut quiet)
        .expect("the linker completes")
}

fn events(division: bool, division_range: f32, merge_range: f32, overlap: f32) -> TrackEvents {
    TrackEvents::new(division, division_range, true, merge_range, overlap)
}

fn detections(centroids: &[[f32; 3]], radius: f32) -> Vec<TrackDetection> {
    centroids
        .iter()
        .map(|centroid| TrackDetection::new(*centroid, [radius; 3]))
        .collect()
}

#[test]
fn measures_a_sheared_placement_on_a_metric_grid() {
    let temp = support::Temp::new("suite-sheared");
    let path = temp.path().join("sheared.ome.zarr");
    let extent = Extent::new(40, 32, 16);
    let spacing = Spacing::new(0.25, 0.25, 0.5);
    let angle = 0.35f32;
    let written = Basis::new([
        [spacing.x() * angle.cos(), spacing.x() * angle.sin(), 0.0],
        [-spacing.y() * angle.sin(), spacing.y() * angle.cos(), 0.0],
        [0.0, 0.0, spacing.z()],
    ]);
    let level = Level::of(extent, written);
    let balls = [[14.0f32, 12.0, 8.0], [26.0, 20.0, 8.0]];
    let channels = [WriteChannel {
        name: "signal",
        tint: [0.2, 0.4, 1.0],
        data: Intensity::new(0.0, 50_000.0),
        window: Intensity::new(0.0, 50_000.0),
    }];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        let origin = block.origin;
        let length = block.extent;
        let mut values = vec![0.0f32; block.voxels()];
        for z in 0..length[2] {
            for y in 0..length[1] {
                for x in 0..length[0] {
                    let index = x as usize
                        + length[0] as usize * (y as usize + length[1] as usize * z as usize);
                    let position = [
                        (origin[0] + x) as f32,
                        (origin[1] + y) as f32,
                        (origin[2] + z) as f32,
                    ];
                    let mut value = 100.0f32;
                    for ball in balls {
                        let squared = (0..3)
                            .map(|axis| (position[axis] - ball[axis]).powi(2))
                            .sum::<f32>();
                        value += 40_000.0 * (-squared / (2.0 * 1.2 * 1.2)).exp();
                    }
                    values[index] = value;
                }
            }
        }
        Ok(VoxelData::F32(values))
    };
    let mut progress = |_, _| {};
    write_image(
        &path,
        &mut ImageSpec {
            name: "sheared",
            channels: &channels,
            levels: &[level],
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the placed field writes");
    shear(&path);
    let image =
        grana_image::Image::open(&Source::file_series(path, 0)).expect("the sheared field opens");
    let source = image.pyramid().level(0);
    assert!(
        !source.basis().orthogonal(),
        "the opened field carries a sheared placement"
    );
    let scene = Scene::new(Dataset::zarr("sheared", image.pyramid().clone()));
    let image = Arc::new(image);
    let render = Channels::render(Arc::clone(&image), &scene);
    assert_eq!(
        render.pyramid().level(0),
        source,
        "the renderer keeps the source lattice"
    );
    let channels = Channels::analysis(Arc::clone(&image), &scene);
    assert!(
        channels.pyramid().level(0).basis().orthogonal(),
        "analysis reads an orthonormal grid"
    );
    let centres = balls.map(|ball| source.voxel_center(ball));
    let spots = spots_of(&channels);
    assert_eq!(spots.len(), 2, "each ball is one spot");
    for centre in centres {
        let nearest = spots
            .iter()
            .map(|spot| {
                (0..3)
                    .map(|axis| (spot.center()[axis] - centre[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest < 0.6,
            "the ball at {centre:?} landed {nearest:.3} away"
        );
    }
}

fn spots_of(channels: &Channels) -> Vec<Spot> {
    let request = grana_pipeline::run::DetectRequest::new(
        0,
        ObjectParams::Spots(SpotParams::new(1.5, 0.01)),
        Region::none(),
        None,
        grana_scene::Drifts::new(),
        Budget::new(1),
        SpillStore::temporary(256 << 20).expect("the geometry store opens"),
    );
    let mut spots = Vec::new();
    let mut sink = |event: grana_pipeline::Event| {
        if let grana_pipeline::Event::Frame {
            frame: ObjectFrame::Spots(set),
            ..
        } = event
        {
            spots.extend_from_slice(set.spots());
        }
    };
    grana_pipeline::run::detect(channels, request, &AtomicBool::new(false), &mut sink)
        .expect("the detection completes");
    spots
}

fn shear(path: &std::path::Path) {
    let store = Arc::new(zarrs::filesystem::FilesystemStore::new(path).expect("the store opens"));
    let group = zarrs::group::Group::open(store.clone(), "/").expect("the root opens");
    let mut attributes = group.attributes().clone();
    attributes["ome"]["multiscales"][0]["coordinateTransformations"][0]["transformations"][1]["affine"] =
        serde_json::json!([
            [0.0, 0.4, 1.0, 0.0],
            [0.0, 0.916_515_1, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ]);
    let mut builder = zarrs::group::GroupBuilder::new();
    builder.attributes(attributes);
    let group = builder.build(store, "/").expect("the root rebuilds");
    group.store_metadata().expect("the sheared metadata stores");
}

#[test]
fn aligns_a_sequence_with_a_deformation_field() {
    let dims = [128usize, 128, 32];
    let extent = Extent::new(128, 128, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.3);
    let reference = speckled(dims);
    let translation = [3.0f32, -2.0, 1.0];
    let field = |at: [f32; 3]| {
        [
            6.0 * (at[0] / 128.0 - 0.5),
            4.0 * (at[1] / 128.0 - 0.5),
            0.0,
        ]
    };
    let frames = [
        reference.clone(),
        moved(&reference, dims, translation),
        warped(&reference, dims, field),
    ]
    .into_iter()
    .map(|values| {
        Volume::new(
            extent,
            spacing,
            vec![Channel::new(
                "signal",
                [0.4, 0.8, 1.0],
                VoxelData::F32(values),
            )],
        )
    })
    .collect();
    let image = Arc::new(
        grana_image::Image::open(&Source::memory(VolumeSeries::new(frames, 1.0)))
            .expect("the synthetic sequence opens"),
    );
    let channels = Channels::stored(image);
    let mut found = None;
    grana_pipeline::run::drift(
        &channels,
        grana_pipeline::DriftRequest::new(0, 0),
        &AtomicBool::new(false),
        &mut |event| {
            if let grana_pipeline::Event::Drift(drift) = event {
                found = Some(drift);
            }
        },
    )
    .expect("the synthetic sequence aligns");
    let drift = found.expect("the alignment reaches its sink");
    assert_eq!(drift.reference(), 0, "the first frame holds still");
    let shift = drift.warp(1).translation();
    for (axis, expected) in shift.iter().zip([0.3f32, -0.2, 0.3]) {
        assert!(
            (axis - expected).abs() < 0.05,
            "the rigid frame reports {shift:?}"
        );
    }
    assert!(
        drift.warp(2).deformed(),
        "the sheared frame carries a displacement field"
    );
    let ranges = [8..120u32, 8..120, 2..30];
    let read = |time: u32, warp: &grana_warp::Warp| {
        let data = channels
            .read_region(time, 0, 0, &ranges, warp)
            .expect("the frame reads");
        let mut values = vec![0.0f32; data.voxel_count()];
        data.write_values(&mut values);
        values
    };
    let still = read(0, &grana_warp::Warp::IDENTITY);
    let loose = read(2, &grana_warp::Warp::IDENTITY);
    let aligned = read(2, drift.warp(2));
    let departed = correlation(&still, &loose);
    let restored = correlation(&still, &aligned);
    assert!(
        departed < 0.9,
        "the sheared frame departs from the reference to {departed}"
    );
    assert!(
        restored > 0.95,
        "the field restores the sheared frame to {restored} from {departed}"
    );
}

fn speckled(dims: [usize; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; dims.iter().product()];
    let mut state = 0x0bad_c0de_u64;
    for value in &mut values {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *value = ((state >> 33) as f32) / ((u32::MAX >> 1) as f32);
    }
    for _ in 0..3 {
        let mut blurred = vec![0.0f32; values.len()];
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let mut sum = 0.0f32;
                    for dz in -1i64..=1 {
                        for dy in -1i64..=1 {
                            for dx in -1i64..=1 {
                                let at = [x as i64 + dx, y as i64 + dy, z as i64 + dz];
                                let at = [
                                    at[0].clamp(0, dims[0] as i64 - 1) as usize,
                                    at[1].clamp(0, dims[1] as i64 - 1) as usize,
                                    at[2].clamp(0, dims[2] as i64 - 1) as usize,
                                ];
                                sum += values[at[0] + dims[0] * (at[1] + dims[1] * at[2])];
                            }
                        }
                    }
                    blurred[x + dims[0] * (y + dims[1] * z)] = sum / 27.0;
                }
            }
        }
        values = blurred;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let spread = (values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f32>()
        / values.len() as f32)
        .sqrt();
    values
        .into_iter()
        .map(|value| 0.5 + 0.2 * (value - mean) / spread)
        .collect()
}

fn warped(source: &[f32], dims: [usize; 3], field: impl Fn([f32; 3]) -> [f32; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; source.len()];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let at = [x as f32, y as f32, z as f32];
                let offset = field(at);
                values[x + dims[0] * (y + dims[1] * z)] = sample(
                    source,
                    dims,
                    [at[0] - offset[0], at[1] - offset[1], at[2] - offset[2]],
                );
            }
        }
    }
    values
}

fn correlation(left: &[f32], right: &[f32]) -> f32 {
    let left_mean = left.iter().sum::<f32>() / left.len() as f32;
    let right_mean = right.iter().sum::<f32>() / right.len() as f32;
    let mut cross = 0.0f64;
    let mut left_square = 0.0f64;
    let mut right_square = 0.0f64;
    for (a, b) in left.iter().zip(right) {
        let a = f64::from(*a - left_mean);
        let b = f64::from(*b - right_mean);
        cross += a * b;
        left_square += a * a;
        right_square += b * b;
    }
    (cross / (left_square * right_square).sqrt()) as f32
}
#[test]
fn renders_and_measures_localizations() {
    let mut noise = support::Noise::new(0x2545_f491);
    let precision = 0.012f32;
    let field = 8.0f32;
    let molecules = (0..600)
        .map(|_| [2.0 + field * noise.value(), 2.0 + field * noise.value()])
        .collect::<Vec<_>>();
    let frames = (0..8)
        .map(|_| {
            molecules
                .iter()
                .map(|molecule| {
                    Spot::gaussian(
                        [
                            molecule[0] + precision * noise.gaussian(),
                            molecule[1] + precision * noise.gaussian(),
                            0.0,
                        ],
                        [precision; 3],
                        900.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let held = support::Localizations::of(frames);
    let spec = LocalizationSpec::new([0, 7], 10.0, 20.0);
    let mut reached = 0usize;
    let report = grana_analysis::localization::localization_report(
        &held,
        spec,
        0.1,
        true,
        None,
        &mut |done, _| {
            reached = done.max(reached);
            true
        },
    )
    .expect("the report renders")
    .expect("the report completes");
    assert!(reached > 0, "the render reports its progress");
    assert_eq!(report.localizations(), 4800);
    let measured = report
        .nena()
        .expect("two frames estimate a precision")
        .precision();
    assert!(
        (measured - 12.0).abs() < 4.0,
        "the precision measures {measured} nm against 12 nm"
    );
    let crowded = (0..600)
        .map(|_| [0.5 + 1.0 * noise.value(), 0.5 + 1.0 * noise.value()])
        .collect::<Vec<_>>();
    let crowded_frames = (0..8)
        .map(|_| {
            crowded
                .iter()
                .map(|molecule| {
                    Spot::gaussian(
                        [
                            molecule[0] + precision * noise.gaussian(),
                            molecule[1] + precision * noise.gaussian(),
                            0.0,
                        ],
                        [precision; 3],
                        900.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let dense = support::Localizations::of(crowded_frames);
    let dense = grana_analysis::localization::localization_report(
        &dense,
        LocalizationSpec::new([0, 7], 10.0, 20.0),
        0.1,
        true,
        None,
        &mut |_, _| true,
    )
    .expect("the dense report renders")
    .expect("the dense report completes");
    let crowded_precision = dense
        .nena()
        .expect("two frames estimate a precision")
        .precision();
    assert!(
        (crowded_precision - 12.0).abs() < 4.0,
        "a crowded field measures {crowded_precision} nm against 12 nm"
    );
    assert!(
        report
            .stack()
            .layer(0)
            .is_some_and(|image| image.size().iter().all(|side| *side > 400)),
        "the image covers the field"
    );
    assert!(
        report
            .stack()
            .layer(0)
            .is_some_and(|image| image.pixels().iter().any(|sample| *sample > 0)),
        "the image paints the localizations"
    );
    assert!(!report.frc().frequencies().is_empty());
    let resolution = report
        .frc()
        .resolution()
        .expect("the rings cross the threshold");
    assert!(
        (10.0..120.0).contains(&resolution),
        "the resolution measures {resolution} nm"
    );
    let geometry = report.geometry();
    assert!((report.pixel() - 10.0).abs() < 1.0e-4);
    assert!((geometry.pixel() - 0.01).abs() < 1.0e-7);
    assert!(geometry.renderable(), "the geometry covers a raster");
    assert!(report.density() > 0.0, "the report normalizes its density");

    let depth = grana_analysis::localization::LocalizationDepth::projection(4.0);
    let width = 4096u32;
    let mut stripes = Vec::with_capacity(width as usize * 3);
    for column in 0..width {
        stripes.extend_from_slice(&[if column % 2 == 0 { 0u8 } else { 255u8 }; 3]);
    }
    let preview = grana_analysis::localization::LocalizationPreview::of_raster(
        [1.0, 2.0],
        0.5,
        [width, 1],
        depth,
        0,
        &stripes,
    );
    assert_eq!(
        preview.size(),
        [2048, 1],
        "a preview bounds its longer side to 2048"
    );
    assert_eq!(preview.origin(), [1.0, 2.0]);
    assert_eq!(preview.extent(), [2048.0, 0.5]);
    let average = (grana_display::linear_to_srgb(0.5) * 255.0).round() as u8;
    assert!(
        average > 128,
        "a preview averages black and white in linear light"
    );
    assert!(
        preview
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| { pixel[..3].iter().all(|value| *value == average) && pixel[3] == 255 }),
        "every preview pixel averages its block"
    );
}

#[test]
fn clusters_localizations_into_molecules() {
    let spot = |x: f32| Spot::new([x, 0.0, 0.0], 0.02, 500.0);
    let frames = [
        vec![spot(0.0), spot(0.02), spot(4.0)],
        vec![spot(0.01)],
        Vec::new(),
        vec![spot(0.0)],
    ];
    let held = support::Localizations::of(frames.clone());
    let spec = grana_object::ClusterSpec::new(0.05, 2, 2);
    let clusters = grana_analysis::localization::cluster(
        &held,
        spec,
        LocalizationFilter::NONE,
        true,
        &mut |_, _| true,
    )
    .expect("the molecules cluster")
    .expect("the molecules complete");
    assert_eq!(
        clusters.molecules(),
        1,
        "the chained spots form one molecule"
    );
    assert_eq!(clusters.localizations(), 4);
    let groups = clusters.groups();
    assert_eq!(groups[0], vec![Some(1), Some(1), None]);
    assert_eq!(groups[1], vec![Some(1)]);
    assert!(groups[2].is_empty());
    assert_eq!(groups[3], vec![Some(1)]);
    let mut rows = Vec::new();
    for (time, frame) in frames.iter().enumerate() {
        for (index, group) in groups[time].iter().enumerate() {
            if let Some(group) = group {
                rows.push((*group, time as u32, index, frame[index].center()));
            }
        }
    }
    let tracks = grana_object::grouped_tracks(1.0, rows).expect("the molecules track");
    let single = tracks
        .tracks()
        .iter()
        .position(|track| track.nodes().len() == 1 && track.start() == 3);
    assert!(
        single.is_some(),
        "a molecule that shows up in one frame keeps its track"
    );
    assert_eq!(
        tracks.track_at(3, 0),
        single,
        "the single-frame detection names its own track"
    );
    let short = grana_object::ClusterSpec::new(0.05, 1, 2);
    let clusters = grana_analysis::localization::cluster(
        &held,
        short,
        LocalizationFilter::NONE,
        true,
        &mut |_, _| true,
    )
    .expect("the short molecules cluster")
    .expect("the short molecules complete");
    assert_eq!(clusters.molecules(), 1);
    assert_eq!(clusters.localizations(), 3);
    assert_eq!(clusters.groups()[3], vec![None]);
}
