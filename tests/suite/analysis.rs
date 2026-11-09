use std::f64::consts::PI;
use std::sync::atomic::AtomicBool;

use grana_analysis::{
    ColocPermutation, ColocScan, ColocScanConfig, FilamentScan, Sources, SpotScan, SurfaceBuilder,
    TrackBuilder, coloc_levels, drift_shift,
};
use grana_geometry::GeometryStore;
use grana_measure::{ColocPair, ColocSeries, ColocSettings, ColocThresholds};
use grana_object::{
    FilamentParams, Spot, SpotParams, SurfaceFilter, SurfaceParams, SurfaceSet, TrackParams,
};
use grana_volume::{
    Basis, ChannelInfo, Extent, Intensity, Level, Pyramid, Region, Spacing, VoxelData, VoxelFormat,
};

use crate::support;

const HIGH: u16 = 65_535;

#[test]
fn detects_and_localises_bright_spots() {
    let extent = Extent::new(96, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let data = spot_volume(extent, spacing);
    let level = Level::new(extent, spacing);
    let span = data.range().span();
    let mut scan = SpotScan::new(level, span, SpotParams::new(0.5, 0.2), Region::none());
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

fn spacing_axis(spacing: Spacing, axis: usize) -> f32 {
    match axis {
        0 => spacing.x(),
        1 => spacing.y(),
        _ => spacing.z(),
    }
}

fn scan_all(scan: &mut SpotScan, data: &VoxelData, level: Level) -> Vec<Spot> {
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
        spots.extend(scan.detect(index, &values, None));
    }
    spots
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
    assert_eq!(series.joint().iter().sum::<u64>(), extent.voxel_count());
    assert_eq!(series.level(), 0);
    let significance = series
        .significance()
        .expect("the series carries a permutation test");
    assert_eq!(significance.randomizations(), 64);
    assert!(
        significance.p() < 0.05,
        "a perfectly correlated pair tests significant"
    );
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
    let levels = coloc_levels(&pyramid, Region::none(), 1);
    let info = pyramid.level(levels.scan());
    let pair = ColocPair::new(0, 1);
    let intensities = [intensity, intensity];
    let settings = ColocSettings::new(64, 3);
    let mut scan = ColocScan::new(
        info,
        &levels,
        ColocScanConfig::new(pair, 2, intensities, None, 1),
        Region::none(),
    );
    let mut permutation = ColocPermutation::new(
        &pyramid,
        Region::none(),
        pair,
        &levels,
        settings,
        intensities,
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
    let loaded = permutation
        .load(
            levels.frames(),
            2,
            &cancel,
            &|_time, channel, ranges| Ok(read(ranges, channel)),
            &|_time| Ok(None),
            &mut quiet,
        )
        .expect("the permutation samples its grid");
    assert!(loaded, "the permutation load is not cancelled");
    let voxels = permutation.sampled_voxels();
    let frames = permutation.sampled_frames();
    let blocks = permutation.blocks();
    let randomizations = permutation.randomizations();
    let significance = permutation.test(2, &cancel, &mut quiet);
    scan.finish(significance, voxels, frames, randomizations, blocks)
}

#[test]
fn links_motion_and_splits_lineages() {
    let mut builder = TrackBuilder::new(TrackParams::new(1.0, 1, false, 2.0));
    builder.push(0, &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]);
    builder.push(1, &[[0.5, 0.0, 0.0]]);
    builder.push(2, &[[1.0, 0.0, 0.0], [5.2, 0.0, 0.0]]);
    let set = builder.finish(1.0);
    assert_eq!(set.len(), 2);
    let moving = set.track_at(2, 0).expect("the moving object is tracked");
    assert_eq!(
        set.track(moving).expect("the track exists").nodes().len(),
        3
    );

    let mut builder = TrackBuilder::new(TrackParams::new(0.8, 0, true, 2.0));
    builder.push(0, &[[0.0, 0.0, 0.0]]);
    builder.push(1, &[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]]);
    builder.push(2, &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    builder.push(3, &[[-1.4, 0.0, 0.0], [1.4, 0.0, 0.0]]);
    let set = builder.finish(2.0);
    assert_eq!(set.len(), 3);
    assert_eq!(set.divisions(), 1);
    assert_eq!(set.children(0).count(), 2);
}
