use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use grana_analysis::{
    ColocScan, ColocScanConfig, FilamentScan, Reference, Sources, SpotScan, SurfaceBuilder,
    TrackLinker, coloc_sampling, drift_shift,
    local::{BlockField, BlockLevels},
};
use grana_geometry::GeometryStore;
use grana_image::Source;
use grana_measure::{ColocPair, ColocSeries, ColocSettings, ColocThresholds};
use grana_object::{
    FilamentParams, Link, Shape, Spot, SpotParams, SurfaceFilter, SurfaceParams, SurfaceSet, Track,
    TrackDetection, TrackEvents, TrackNode, TrackParams, TrackSet,
};
use grana_pipeline::Channels;
use grana_scene::{Dataset, Drift, ObjectFrame, ObjectParams, Scene};
use grana_volume::{
    Basis, ChannelInfo, Extent, Intensity, Level, Pyramid, Region, Spacing, VoxelData, VoxelFormat,
};
use grana_zarr::{ImageSpec, WriteChannel, write_image};

use crate::support;

const HIGH: u16 = 65_535;

#[test]
fn detects_and_localises_bright_spots() {
    let extent = Extent::new(96, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let data = spot_volume(extent, spacing);
    let level = Level::new(extent, spacing);
    let mut scan = SpotScan::new(
        level,
        data.range(),
        SpotParams::new(0.5, 0.2),
        Region::none(),
    );
    let spots = scan_all(&mut scan, &data, level);
    assert_eq!(spots.len(), 2, "each bright blob is one spot");
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

#[test]
fn keeps_spot_contrast_comparable_across_a_background_gradient() {
    let extent = Extent::new(64, 64, 16);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let data = gradient_spot_volume(extent, spacing);
    let level = Level::new(extent, spacing);
    let mut scan = SpotScan::new(
        level,
        data.range(),
        SpotParams::new(0.5, 0.2),
        Region::none(),
    );
    let spots = scan_all(&mut scan, &data, level);
    assert_eq!(spots.len(), 2, "both blobs clear the local contrast");
    let quality = spots
        .iter()
        .map(|spot| spot.quality())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| {
            (low.min(value), high.max(value))
        });
    assert!(
        quality.1 - quality.0 < 0.2,
        "the local contrast of the blobs stays comparable: {quality:?}"
    );
}

fn gradient_spot_volume(extent: Extent, spacing: Spacing) -> VoxelData {
    let sigma = 0.5 / 2.35482;
    let mut values = vec![0.0f32; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                values[extent.index(x, y, z) as usize] = 600.0 + 15.0 * x as f32;
            }
        }
    }
    for centre in [[16.0f32, 32.0, 8.0], [48.0, 32.0, 8.0]] {
        for z in 0..extent.z() {
            for y in 0..extent.y() {
                for x in 0..extent.x() {
                    let offset = [
                        (x as f32 - centre[0]) * spacing.x(),
                        (y as f32 - centre[1]) * spacing.y(),
                        (z as f32 - centre[2]) * spacing.z(),
                    ];
                    let squared = offset.iter().map(|value| value * value).sum::<f32>();
                    let value = 250.0 * (-squared / (2.0 * sigma * sigma)).exp();
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

fn scan_all(scan: &mut SpotScan, data: &VoxelData, level: Level) -> Vec<Spot> {
    let field = spot_field(data, level);
    let mut spots = Vec::new();
    let mut values = Vec::new();
    for index in 0..scan.block_count() {
        let range = scan.read_range(index);
        let region = data.region(
            level.extent(),
            range[0].clone(),
            range[1].clone(),
            range[2].clone(),
        );
        values.clear();
        values.resize(region.voxel_count(), 0.0);
        region.write_values(&mut values);
        spots.extend(scan.detect(index, &values, &field, None));
    }
    spots
}

fn spot_field(data: &VoxelData, level: Level) -> BlockField {
    let extent = level.extent();
    let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let mut levels = BlockLevels::new([0, 0, 0], ranges.clone(), ranges.clone(), data.range(), 1);
    let mut plane = Vec::new();
    for z in ranges[2].start..ranges[2].end {
        let region = data.region(extent, ranges[0].clone(), ranges[1].clone(), z..z + 1);
        plane.clear();
        plane.resize(region.voxel_count(), 0.0);
        region.write_values(&mut plane);
        levels.push(&|_| true, z, &plane, 1);
    }
    levels.finish()
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
}

#[test]
fn places_measured_surfaces_through_the_placement_basis() {
    let extent = Extent::new(80, 64, 48);
    let spacing = Spacing::new(0.2, 0.2, 0.2);
    let centres = [[24.0f32, 32.0, 24.0], [56.0, 32.0, 24.0]];
    let volume = support::spheres(
        extent,
        spacing,
        &[(centres[0], 8.0), (centres[1], 8.0)],
        HIGH,
    );
    let level = Level::of(
        extent,
        Basis::new([[0.0, -0.2, 0.0], [0.2, 0.0, 0.0], [0.0, 0.0, 0.3]]),
    );
    let set = extract_surfaces_at(
        &volume,
        level,
        SurfaceParams::new(0.5, 0.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0)),
    );
    assert_eq!(set.object_count(), 2, "each ball is one object");
    let expected = centres.map(|centre| level.voxel_center(centre));
    for object in set.objects() {
        let centre = object.centroid();
        let placed = expected.iter().any(|candidate| {
            (0..3)
                .map(|axis| f64::from(centre[axis] - candidate[axis]).powi(2))
                .sum::<f64>()
                < 0.25
        });
        assert!(placed, "the object centroid {centre:?} is misplaced");
    }
}

fn extract_surfaces(volume: &grana_volume::Volume, params: SurfaceParams) -> SurfaceSet {
    extract_surfaces_at(
        volume,
        Level::new(volume.extent(), volume.spacing()),
        params,
    )
}

fn extract_surfaces_at(
    volume: &grana_volume::Volume,
    level: Level,
    params: SurfaceParams,
) -> SurfaceSet {
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = volume.channel_count();
    let builder = SurfaceBuilder::new(0, level, params, channels, 0, Region::none(), intensity);
    let cancel = AtomicBool::new(false);
    builder
        .scan(
            4,
            &cancel,
            None,
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
                &GeometryStore::memory(),
            ),
        )
        .expect("a surface frame is detected")
        .expect("surface detection completes")
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
}

#[test]
fn exports_relation_pairs() {
    let source = relation_surfaces(&[([20.0, 32.0, 24.0], 8.0)]);
    let target = relation_surfaces(&[([26.0, 32.0, 24.0], 8.0)]);
    let frame = Reference::new(&Shape::mesh(
        target.geometry().expect("the target carries geometry"),
    ))
    .relate(
        &Shape::mesh(source.geometry().expect("the source carries geometry")),
        0.0,
        false,
    );
    let mut export = grana_measure::PairExport::new(0.5, "cells", "nuclei");
    export.push(3, &frame);
    let text = export.text();
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            "frame,time_s,source_set,target_set,source,target,contacting,contact_um2,distance_um,inside_percent,center_um,source_partners"
        ),
        "the pair export names every column"
    );
    let row = lines.next().expect("the relation writes one pair");
    assert!(lines.next().is_none(), "the frame holds one pair");
    let values = row.split(',').collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            "4", "1.500", "cells", "nuclei", "1", "1", "0", values[7], values[8], values[9],
            values[10], "1",
        ],
        "the pair names its frame, offset and objects"
    );
    assert_eq!(values[7], "0.000", "a nested pair touches no surface area");
    assert_eq!(values[8], "0.000", "an overlapping pair touches");
    assert!(
        values[9].parse::<f32>().expect("the containment parses") > 30.0,
        "the pair carries the containment of its source"
    );
    assert_eq!(
        grana_measure::PairExport::file_name("cells", "nuclei", -1),
        "relations-cells-nuclei-t-1.csv"
    );
}

fn relation_surfaces(balls: &[([f32; 3], f32)]) -> SurfaceSet {
    let volume = support::spheres(
        Extent::new(64, 64, 48),
        Spacing::new(0.2, 0.2, 0.2),
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
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let offset = ((y as f32 - 20.0).powi(2) + (z as f32 - 15.0).powi(2)).sqrt();
                let intensity = 300.0 + 4_000.0 * (-0.5 * (offset / 0.9).powi(2)).exp();
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
        1,
        0,
        Region::none(),
        intensity,
    );
    let set = scan
        .frame(&data, None, &GeometryStore::memory(), |_| Ok(data.clone()))
        .expect("the filament scan traces the frame");
    assert_eq!(set.object_count(), 1, "the dim filament survives");
    let length = set.object(0).expect("the filament exists").length();
    let expected = f64::from(extent.x()) * f64::from(spacing.x());
    assert!(
        length > 0.5 * expected,
        "the traced length {length:.2} keeps most of {expected:.2}"
    );
}

#[test]
fn recovers_a_translation_and_reports_a_still_volume() {
    let dims = [32usize, 32, 16];
    let reference = texture(dims);
    for offset in [[3.0, -2.0, 1.0], [-4.0, 5.0, -2.0], [2.5, -1.25, 0.5]] {
        let moving = moved(&reference, dims, offset);
        let found = drift_shift(&reference, &moving, dims);
        for axis in 0..3 {
            assert!(
                (found[axis] - offset[axis]).abs() < 0.2,
                "the estimator recovers {offset:?} but reported {found:?}"
            );
        }
    }
    let still = vec![4.0f32; dims.iter().product()];
    assert_eq!(drift_shift(&still, &still, dims), [0.0; 3]);
}

fn texture(dims: [usize; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; dims.iter().product()];
    let mut state = 0x5eed_1234_u64;
    let mut random = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as f32) / ((u32::MAX >> 1) as f32) - 0.5
    };
    for (center, sigma, weight) in [
        ([10.0f32, 9.0, 6.0], 2.5f32, 1.0f32),
        ([20.0, 20.0, 10.0], 3.5, 0.7),
        ([15.0, 22.0, 12.0], 2.2, 1.4),
    ] {
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let dx = x as f32 - center[0];
                    let dy = y as f32 - center[1];
                    let dz = z as f32 - center[2];
                    let squared = dx * dx + dy * dy + dz * dz;
                    let envelope = weight * (-squared / (2.0 * sigma * sigma)).exp();
                    values[x + dims[0] * (y + dims[1] * z)] += envelope * (0.6 + 0.8 * random());
                }
            }
        }
    }
    values
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
    let series = coloc_series(extent, &first, &first);
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
}

#[test]
fn chooses_the_colocalization_precision() {
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let extents = [256, 128, 64, 32].map(|side| Extent::new(side, side, side));
    let levels = extents
        .iter()
        .map(|extent| Level::new(*extent, spacing))
        .collect::<Vec<_>>();
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channel = ChannelInfo::new(
        "Channel".to_string(),
        [1.0, 1.0, 1.0],
        intensity,
        intensity,
        VoxelFormat::U16,
    );
    let pyramid = Pyramid::new(levels, 1, vec![channel], 1, 1.0);
    let full = coloc_sampling(
        &pyramid,
        Region::none(),
        grana_measure::ColocPrecision::Full,
        3,
    );
    let balanced = coloc_sampling(
        &pyramid,
        Region::none(),
        grana_measure::ColocPrecision::Balanced,
        3,
    );
    let fast = coloc_sampling(
        &pyramid,
        Region::none(),
        grana_measure::ColocPrecision::Fast,
        3,
    );
    assert_eq!(full.level(), 0);
    assert_eq!(balanced.level(), 1);
    assert_eq!(fast.level(), 2);
    assert!(full.stride() >= balanced.stride());
    assert!(balanced.stride() >= fast.stride());
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

fn coloc_series(extent: Extent, first: &VoxelData, second: &VoxelData) -> ColocSeries {
    let spacing = Spacing::new(0.1, 0.1, 0.3);
    let level = Level::new(extent, spacing);
    let intensity = Intensity::new(0.0, f32::from(HIGH));
    let channels = (0..2)
        .map(|channel| {
            ChannelInfo::new(
                format!("Channel {channel}"),
                [1.0, 1.0, 1.0],
                intensity,
                intensity,
                VoxelFormat::U16,
            )
        })
        .collect();
    let pyramid = Pyramid::new(vec![level], 1, channels, 1, 1.0);
    let settings = ColocSettings::new(64, 3);
    let sampling = coloc_sampling(
        &pyramid,
        Region::none(),
        settings.precision(),
        settings.block(),
    );
    let info = pyramid.level(sampling.level());
    let pair = ColocPair::new(0, 1);
    let intensities = [intensity, intensity];
    let mut scan = ColocScan::new(
        info,
        sampling,
        ColocScanConfig::new(pair, 2, intensities, None),
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
fn links_motion_and_splits_lineages() {
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

    let set = tracked(
        &[
            (&[[0.0, 0.0, 0.0]][..], 1.0),
            (&[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]][..], 0.8),
            (&[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]][..], 0.8),
            (&[[-1.4, 0.0, 0.0], [1.4, 0.0, 0.0]][..], 0.8),
        ],
        TrackParams::new(0.8, 0, events(true, 2.0, 2.0, 0.0)),
        2.0,
    );
    assert_eq!(set.len(), 3);
    assert_eq!(set.divisions(), 1);
    assert_eq!(set.children(0).count(), 2);
}

#[test]
fn rejects_a_division_whose_daughters_do_not_persist() {
    let set = tracked(
        &[
            (&[[0.0, 0.0, 0.0]][..], 1.0),
            (&[[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0]][..], 0.8),
        ],
        TrackParams::new(1.5, 0, events(true, 2.5, 2.5, 0.0)),
        1.0,
    );
    assert_eq!(set.divisions(), 0);
}

#[test]
fn splits_when_the_daughters_outlive_the_volume_gate() {
    let frames = [
        (&[[0.0, 0.0, 0.0]][..], 1.0),
        (&[[-0.55, 0.0, 0.0], [0.55, 0.0, 0.0]][..], 0.55),
        (&[[-0.65, 0.0, 0.0], [0.65, 0.0, 0.0]][..], 0.55),
        (&[[-0.75, 0.0, 0.0], [0.75, 0.0, 0.0]][..], 0.55),
    ];
    let set = tracked(
        &frames,
        TrackParams::new(1.5, 0, events(true, 2.5, 2.5, 0.0)),
        1.0,
    );
    assert_eq!(set.divisions(), 1);
    let persistent = tracked(
        &frames[..2],
        TrackParams::new(1.5, 0, events(true, 2.5, 2.5, 0.0)),
        1.0,
    );
    assert_eq!(persistent.divisions(), 0);
}

#[test]
fn prefers_the_daughters_that_carry_the_sequence() {
    let set = tracked(
        &[
            (&[[0.0, 0.0, 0.0]][..], 1.3),
            (
                &[[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [1.9, 0.0, 0.0]][..],
                0.8,
            ),
            (&[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]][..], 0.8),
            (&[[-0.7, 0.0, 0.0], [0.7, 0.0, 0.0]][..], 0.8),
        ],
        TrackParams::new(1.5, 0, events(true, 2.5, 2.5, 0.0)),
        1.0,
    );
    assert_eq!(set.divisions(), 1);
    let mother = set.track_at(0, 0).expect("the mother is tracked");
    let started = set
        .children(mother)
        .map(|child| set.track(child).expect("a child track exists").nodes()[0].time())
        .collect::<Vec<_>>();
    assert_eq!(started, vec![1, 1]);
    let frames = set
        .children(mother)
        .map(|child| {
            set.track(child)
                .expect("a child track exists")
                .nodes()
                .len()
        })
        .collect::<Vec<_>>();
    assert!(
        frames.iter().all(|frames| *frames >= 3),
        "the child lengths are {frames:?}"
    );
}

#[test]
fn merges_two_lineages_that_share_a_detection() {
    let frames = || {
        [
            (&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]][..], 1.2),
            (&[[1.0, 0.0, 0.0]][..], 1.5),
            (&[[1.2, 0.0, 0.0]][..], 1.5),
        ]
    };
    let set = tracked(
        &frames(),
        TrackParams::new(2.0, 0, events(false, 2.0, 2.0, 0.0)),
        1.0,
    );
    assert_eq!(set.merges(), 1);
    let merged = set.track_at(2, 0).expect("the merged object is tracked");
    assert_eq!(set.parents(merged).count(), 2);
    assert!(set.parents(merged).all(|link| link.kind() == Link::Merge));
    assert_eq!(
        set.track(merged)
            .expect("the merged track exists")
            .nodes()
            .len(),
        2
    );

    let apart = tracked(
        &frames(),
        TrackParams::new(2.0, 0, events(false, 2.0, 2.0, 0.35)),
        1.0,
    );
    assert_eq!(apart.merges(), 0);

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
        .map(|centroid| TrackDetection::new(*centroid, radius))
        .collect()
}

#[test]
fn joins_two_earlier_tracks_into_a_later_start() {
    let set = TrackSet::new(
        1.0,
        vec![
            Track::new(vec![TrackNode::new(0, 0, [0.0, 0.0, 0.0])]),
            Track::new(vec![TrackNode::new(0, 1, [2.0, 0.0, 0.0])]),
            Track::new(vec![
                TrackNode::new(1, 0, [1.0, 0.0, 0.0]),
                TrackNode::new(2, 0, [1.1, 0.0, 0.0]),
            ]),
        ],
    );
    assert!(set.can_merge((0, 0), (0, 1), (1, 0)));
    let merged = set
        .merge((0, 0), (0, 1), (1, 0))
        .expect("two earlier tracks merge into a later start");
    assert_eq!(merged.merges(), 1);
    assert_eq!(merged.links().len(), 2);
    assert!(merged.links().iter().all(|link| link.kind() == Link::Merge));
    assert_eq!(merged.parents(2).count(), 2);
    assert!(!merged.can_merge((0, 0), (1, 0), (2, 0)));
    assert!(merged.merge((0, 0), (0, 1), (1, 0)).is_none());
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
        range: Intensity::new(0.0, 50_000.0),
    }];
    let mut fill = |_level: usize,
                    _frame: u32,
                    _channel: usize,
                    origin: [u32; 3],
                    length: [u32; 3],
                    data: &mut [f32]|
     -> Result<(), String> {
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
                    data[index] = value;
                }
            }
        }
        Ok(())
    };
    let mut progress = |_, _| {};
    write_image(
        &path,
        ImageSpec {
            name: "sheared",
            channels: &channels,
            levels: &[level],
            placement: [0.0; 3],
            frames: 1,
            interval: 1.0,
            fill: &mut fill,
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
        Drift::default(),
        1,
        GeometryStore::temporary(256 << 20).expect("the geometry store opens"),
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
fn measures_a_relaxed_label_surface() {
    let extent = Extent::new(48, 48, 48);
    let spacing = Spacing::new(1.0, 1.0, 1.0);
    let radius = 10.0f32;
    let centre = [24.0f32, 24.0, 24.0];
    let mut labels = vec![0u32; 48 * 48 * 48];
    let mut channel = vec![0.0f32; 48 * 48 * 48];
    for z in 0..48u32 {
        for y in 0..48u32 {
            for x in 0..48u32 {
                let slot = (x + 48 * (y + 48 * z)) as usize;
                let point = [x as f32, y as f32, z as f32];
                let distance = (0..3)
                    .map(|axis| (point[axis] - centre[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                if distance <= radius {
                    labels[slot] = 1;
                    channel[slot] = 1.0;
                }
            }
        }
    }
    let level = Level::new(extent, spacing);
    let mut builder = grana_analysis::LabelBuilder::new(0, level, 1);
    builder.push(grana_analysis::LabelSlab {
        origin: [0, 0, 0],
        core: [48, 48, 48],
        extent: [48, 48, 48],
        labels: &labels,
        channels: &[VoxelData::F32(channel)],
    });
    let set = builder.finish().expect("the label surface is built");
    let object = set.objects()[0];
    let expected_volume = 4.0 / 3.0 * PI * f64::from(radius).powi(3);
    let expected_area = 4.0 * PI * f64::from(radius).powi(2);
    assert!(
        (object.volume() - expected_volume).abs() / expected_volume < 0.05,
        "the sphere volume {} is off from {expected_volume}",
        object.volume()
    );
    assert!(
        (object.area() - expected_area).abs() / expected_area < 0.05,
        "the sphere area {} is off from {expected_area}",
        object.area()
    );
    assert!(
        object.sphericity() > 0.97,
        "a voxelised sphere surfaces with sphericity {}",
        object.sphericity()
    );
}
