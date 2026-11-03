use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::{ColocPermutation, ColocScan, ColocScanConfig, Roi, Shape, coloc_levels};
use grana_measure::{
    COLOC_BINS_MAX, COLOC_BINS_MIN, ColocPair, ColocSeries, ColocSettings, ColocThresholds,
};
use grana_object::Spot;
use grana_volume::{
    ChannelInfo, Extent, Intensity, Level, Pyramid, Region, Spacing, VoxelData, VoxelFormat,
};

const EXTENT: Extent = Extent::new(64, 48, 8);
const RAMP: f32 = 65535.0;

#[test]
fn measures_correlated_channels() {
    let first = volume(EXTENT, |x, _, _| x as f32 / (EXTENT.x() - 1) as f32);
    let second = first.clone();
    let series = scan(
        EXTENT,
        1,
        Region::none(),
        &[first],
        &[second],
        None,
        ColocSettings::default(),
    );
    let statistics = series.statistics(ColocThresholds::new(0.5, 0.5));
    assert_eq!(statistics.voxels(), EXTENT.voxel_count());
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
    let manders = statistics.manders();
    assert!(manders[0].is_some_and(|value| (value - manders[1].unwrap()).abs() < 1.0e-6));
    let bins = series.bins();
    let cut = ((0.5f32 * bins as f32) as usize).min(bins - 1);
    let bin =
        |x: u32| (((x as f32 / (EXTENT.x() - 1) as f32) * bins as f32) as usize).min(bins - 1);
    let above = (0..EXTENT.x())
        .filter(|x| bin(*x) > cut)
        .map(|x| x as f64)
        .sum::<f64>();
    let total = (0..EXTENT.x()).map(|x| x as f64).sum::<f64>();
    assert!((manders[0].unwrap() - above / total).abs() < 1.0e-3);
    let otsu = series.otsu();
    assert!(
        (0.3..0.7).contains(&otsu.a()),
        "the otsu threshold splits the ramp near its middle"
    );
    assert!((COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&series.bins()));
    assert_eq!(series.joint().len(), series.bins() * series.bins());
    assert_eq!(series.joint().iter().sum::<u64>(), EXTENT.voxel_count());
    assert_eq!(series.level(), 0);
    assert_eq!(series.permutation_level(), 0);
    assert_eq!(series.cost().scan_voxels(), EXTENT.voxel_count());
    let significance = series
        .significance()
        .expect("the series carries a permutation test");
    assert_eq!(significance.randomizations(), 200);
    assert!(
        significance.p() < 0.01,
        "a perfectly correlated pair tests significant"
    );
}

#[test]
fn measures_anticorrelated_channels_inside_a_region() {
    let first = volume(EXTENT, |x, _, _| x as f32 / (EXTENT.x() - 1) as f32);
    let second = volume(EXTENT, |x, _, _| 1.0 - x as f32 / (EXTENT.x() - 1) as f32);
    let region = Region::new(Vec3::new(-1.0, -1.0, -2.0), Vec3::new(1.0, 1.0, 2.0));
    let series = scan(
        EXTENT,
        1,
        region,
        &[first],
        &[second],
        None,
        ColocSettings::default(),
    );
    let statistics = series.statistics(ColocThresholds::new(0.5, 0.5));
    assert!(
        statistics.voxels() < EXTENT.voxel_count(),
        "the region carries fewer voxels than the volume"
    );
    assert!(
        statistics.pearson().is_some_and(|value| value < -0.999),
        "opposite ramps anticorrelate"
    );
    let significance = series
        .significance()
        .expect("the series carries a permutation test");
    assert!(
        significance.p() > 0.05,
        "anticorrelated channels are not significantly colocalized"
    );
}

#[test]
fn gates_the_scan_by_the_analysis_mask() {
    let spots: Arc<[Spot]> = vec![Spot::new([0.0, 0.0, 0.0], 1.5, 1.0)].into();
    let roi = Roi::new(vec![Some(Shape::spots(spots))]);
    let first = volume(EXTENT, |x, _, _| x as f32 / (EXTENT.x() - 1) as f32);
    let second = volume(EXTENT, |x, y, z| {
        let unit = x as f32 / (EXTENT.x() - 1) as f32;
        if inside(x, y, z) { unit } else { 1.0 - unit }
    });
    let masked = scan(
        EXTENT,
        1,
        Region::none(),
        std::slice::from_ref(&first),
        std::slice::from_ref(&second),
        Some(&roi),
        ColocSettings::default(),
    );
    let whole = scan(
        EXTENT,
        1,
        Region::none(),
        &[first],
        &[second],
        None,
        ColocSettings::default(),
    );
    let mask = roi.mask(level(EXTENT), 0).expect("the mask rasterizes");
    let masked_statistics = masked.statistics(ColocThresholds::new(0.5, 0.5));
    assert_eq!(masked_statistics.voxels(), mask.count());
    assert!(
        masked_statistics
            .pearson()
            .is_some_and(|value| value > 0.95),
        "the masked core correlates"
    );
    let whole_statistics = whole.statistics(ColocThresholds::new(0.5, 0.5));
    assert!(
        whole_statistics
            .pearson()
            .is_some_and(|value| value < -0.05),
        "the anticorrelated surroundings dominate the unmasked volume"
    );
    assert!(
        masked
            .significance()
            .is_some_and(|significance| significance.p() < 0.01)
    );
    assert!(
        whole
            .significance()
            .is_some_and(|significance| significance.p() > 0.05)
    );
}

#[test]
fn keeps_the_series_identical_across_worker_counts() {
    let pyramid = pyramid(EXTENT, 3);
    let first = (0..3)
        .map(|frame| volume(EXTENT, move |x, y, z| hash(x, y, z, 1 + frame)))
        .collect::<Vec<_>>();
    let second = (0..3)
        .map(|frame| volume(EXTENT, move |x, y, z| hash(x, y, z, 7 + frame)))
        .collect::<Vec<_>>();
    let data = Channels {
        first: &first,
        second: &second,
    };
    let settings = ColocSettings::new(64, 3);
    let setup = |workers| {
        measured(
            Setup {
                pyramid: &pyramid,
                frames: 3,
                region: Region::none(),
                scan: &data,
                permutation: &data,
                roi: None,
                settings,
            },
            workers,
        )
    };
    assert_eq!(setup(1), setup(8));
}

#[test]
fn adapts_the_bin_count_to_the_sample() {
    let small = Extent::new(24, 24, 24);
    let large = Extent::new(48, 48, 48);
    let settings = ColocSettings::new(8, 3);
    let small_series = scan(
        small,
        1,
        Region::none(),
        &[volume(small, |x, _, _| x as f32 / (small.x() - 1) as f32)],
        &[volume(small, |x, _, _| x as f32 / (small.x() - 1) as f32)],
        None,
        settings,
    );
    let large_series = scan(
        large,
        1,
        Region::none(),
        &[volume(large, |x, _, _| x as f32 / (large.x() - 1) as f32)],
        &[volume(large, |x, _, _| x as f32 / (large.x() - 1) as f32)],
        None,
        settings,
    );
    assert!((COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&small_series.bins()));
    assert!((COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&large_series.bins()));
    assert!(
        large_series.bins() > small_series.bins(),
        "a larger sample resolves more intensity bins"
    );
}

#[test]
fn selects_pyramid_levels_within_the_budgets() {
    let pyramid = described_pyramid(1);
    let levels = coloc_levels(&pyramid, Region::none(), 1);
    assert_eq!(levels.scan(), 1);
    assert_eq!(levels.permutation(), 3);
    assert_eq!(levels.stride(), 1);
    assert_eq!(levels.frames(), &[0]);

    let pyramid = described_pyramid(40);
    let levels = coloc_levels(&pyramid, Region::none(), 40);
    assert_eq!(levels.scan(), 1);
    assert_eq!(levels.permutation(), 3);
    assert_eq!(levels.stride(), 1);
    assert_eq!(levels.frames().len(), 8);
}

#[test]
fn samples_a_coarser_permutation_level() {
    let scan_extent = Extent::new(64, 48, 8);
    let permutation_extent = Extent::new(32, 24, 8);
    let frames = 8;
    let pyramid = Pyramid::new(
        vec![
            level(scan_extent),
            Level::new(permutation_extent, Spacing::new(0.2, 0.2, 0.6)),
        ],
        channels(),
        frames,
        1.0,
    );
    let scan_first = (0..frames)
        .map(|_| {
            volume(scan_extent, |x, _, _| {
                x as f32 / (scan_extent.x() - 1) as f32
            })
        })
        .collect::<Vec<_>>();
    let scan_second = scan_first.clone();
    let permutation_first = (0..frames)
        .map(|_| {
            volume(permutation_extent, |x, _, _| {
                x as f32 / (permutation_extent.x() - 1) as f32
            })
        })
        .collect::<Vec<_>>();
    let permutation_second = permutation_first.clone();
    let series = measured(
        Setup {
            pyramid: &pyramid,
            frames,
            region: Region::none(),
            scan: &Channels {
                first: &scan_first,
                second: &scan_second,
            },
            permutation: &Channels {
                first: &permutation_first,
                second: &permutation_second,
            },
            roi: None,
            settings: ColocSettings::new(40, 3),
        },
        4,
    );
    assert_eq!(series.level(), 0);
    assert_eq!(series.permutation_level(), 1);
    assert_eq!(series.permutation_stride(), 1);
    assert!(
        series
            .significance()
            .is_some_and(|significance| significance.p() < 0.05)
    );
}

#[test]
fn samples_a_stride_when_no_coarser_level_fits() {
    let extent = Extent::new(96, 96, 64);
    let frames = 1;
    let pyramid = pyramid(extent, frames);
    let levels = coloc_levels(&pyramid, Region::none(), frames);
    assert_eq!(levels.scan(), 0);
    assert_eq!(levels.permutation(), 0);
    assert_eq!(levels.stride(), 2);
    let first = volume(extent, |x, _, _| x as f32 / (extent.x() - 1) as f32);
    let second = first.clone();
    let series = scan(
        extent,
        frames,
        Region::none(),
        &[first],
        &[second],
        None,
        ColocSettings::new(40, 3),
    );
    assert_eq!(series.permutation_stride(), 2);
    assert!(
        series
            .significance()
            .is_some_and(|significance| significance.p() < 0.05)
    );
}

#[test]
fn samples_a_grid_that_does_not_divide_the_blocks() {
    let extent = Extent::new(61, 37, 23);
    let settings = ColocSettings::new(200, 3);
    let identical = volume(extent, |x, _, _| x as f32 / (extent.x() - 1) as f32);
    let correlated = scan(
        extent,
        1,
        Region::none(),
        std::slice::from_ref(&identical),
        std::slice::from_ref(&identical),
        None,
        settings,
    );
    assert_eq!(correlated.permutation_level(), 0);
    assert_eq!(correlated.permutation_stride(), 1);
    assert!((COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&correlated.bins()));
    let significance = correlated
        .significance()
        .expect("the correlated grid carries a permutation test");
    assert_eq!(significance.randomizations(), 200);
    assert!(
        significance.p() < 0.01,
        "an identical pair tests significant on a grid that does not divide the blocks"
    );

    let first = volume(extent, |x, y, z| hash(x, y, z, 1));
    let second = volume(extent, |x, y, z| hash(x, y, z, 2));
    let independent = scan(
        extent,
        1,
        Region::none(),
        &[first],
        &[second],
        None,
        settings,
    );
    let significance = independent
        .significance()
        .expect("the independent grid carries a permutation test");
    assert!(
        significance.p() > 0.05,
        "independent noise is not significant on a grid that does not divide the blocks"
    );
}

#[test]
fn finds_the_costes_threshold_interval() {
    let first = volume(EXTENT, |x, y, _| {
        if core(x, y) {
            0.6 + 0.4 * x as f32 / (EXTENT.x() - 1) as f32
        } else {
            0.05 + 0.15 * x as f32 / (EXTENT.x() - 1) as f32
        }
    });
    let second = volume(EXTENT, |x, y, _| {
        if core(x, y) {
            0.6 + 0.4 * x as f32 / (EXTENT.x() - 1) as f32
        } else {
            0.05 + 0.15 * y as f32 / (EXTENT.y() - 1) as f32
        }
    });
    let series = scan(
        EXTENT,
        1,
        Region::none(),
        &[first],
        &[second],
        None,
        ColocSettings::new(8, 3),
    );
    let costes = series.costes().expect(
        "a correlated core separated from an uncorrelated background carries Costes thresholds",
    );
    let high = costes.high();
    assert!(high.a() > 0.3 && high.a() < 0.9, "{high:?}");
    assert!(costes.low().a() <= high.a() && costes.low().b() <= high.b());
    let manders = series.statistics(high).manders();
    assert!(manders[0].is_some_and(|value| value > 0.5), "{manders:?}");
    assert!(manders[1].is_some_and(|value| value > 0.5), "{manders:?}");
}

struct Channels<'a> {
    first: &'a [VoxelData],
    second: &'a [VoxelData],
}

struct Setup<'a> {
    pyramid: &'a Pyramid,
    frames: u32,
    region: Region,
    scan: &'a Channels<'a>,
    permutation: &'a Channels<'a>,
    roi: Option<&'a Roi>,
    settings: ColocSettings,
}

fn scan(
    extent: Extent,
    frames: u32,
    region: Region,
    first: &[VoxelData],
    second: &[VoxelData],
    roi: Option<&Roi>,
    settings: ColocSettings,
) -> ColocSeries {
    let pyramid = pyramid(extent, frames);
    let data = Channels { first, second };
    measured(
        Setup {
            pyramid: &pyramid,
            frames,
            region,
            scan: &data,
            permutation: &data,
            roi,
            settings,
        },
        4,
    )
}

fn measured(setup: Setup<'_>, workers: usize) -> ColocSeries {
    let Setup {
        pyramid,
        frames,
        region,
        scan: scan_data,
        permutation: permutation_data,
        roi,
        settings,
    } = setup;
    let levels = coloc_levels(pyramid, region, frames);
    let info = pyramid.level(levels.scan());
    let scan_extent = info.extent();
    let pair = ColocPair::new(0, 1);
    let intensities = [intensity(), intensity()];
    let mut scan = ColocScan::new(
        info,
        &levels,
        ColocScanConfig::new(pair, 2, intensities, None, frames),
        region,
    );
    let mut permutation =
        ColocPermutation::new(pyramid, region, pair, &levels, settings, intensities);
    let cancel = AtomicBool::new(false);
    let mut quiet = |_: usize, _: usize| {};
    let scanned = scan
        .scan(
            workers,
            &cancel,
            &|time, channel, ranges| {
                let data = if channel == pair.a() {
                    &scan_data.first[time as usize]
                } else {
                    &scan_data.second[time as usize]
                };
                Ok(slice(data, scan_extent, ranges))
            },
            &|time| Ok(roi.map(|roi| roi.mask(info, time).expect("the mask rasterizes"))),
            &mut quiet,
        )
        .expect("the scan completes");
    assert!(scanned, "the scan is not cancelled");
    let permutation_info = pyramid.level(levels.permutation());
    let permutation_extent = permutation_info.extent();
    let loaded = permutation
        .load(
            levels.frames(),
            workers,
            &cancel,
            &|time, channel, ranges| {
                let data = if channel == pair.a() {
                    &permutation_data.first[time as usize]
                } else {
                    &permutation_data.second[time as usize]
                };
                Ok(slice(data, permutation_extent, ranges))
            },
            &|time| {
                Ok(roi.map(|roi| {
                    roi.mask(permutation_info, time)
                        .expect("the mask rasterizes")
                }))
            },
            &mut quiet,
        )
        .expect("the permutation samples its grid");
    assert!(loaded, "the permutation load is not cancelled");
    let permutation_voxels = permutation.sampled_voxels();
    let permutation_frames = permutation.sampled_frames();
    let blocks = permutation.blocks();
    let randomizations = permutation.randomizations();
    let significance = permutation.test(workers, &cancel, &mut quiet);
    scan.finish(
        significance,
        permutation_voxels,
        permutation_frames,
        randomizations,
        blocks,
    )
}

fn level(extent: Extent) -> Level {
    Level::new(extent, Spacing::new(0.1, 0.1, 0.3))
}

fn pyramid(extent: Extent, frames: u32) -> Pyramid {
    Pyramid::new(vec![level(extent)], channels(), frames, 1.0)
}

fn described_pyramid(frames: u32) -> Pyramid {
    let levels = vec![
        Level::new(Extent::new(512, 512, 64), Spacing::new(0.1, 0.1, 0.3)),
        Level::new(Extent::new(256, 256, 32), Spacing::new(0.2, 0.2, 0.6)),
        Level::new(Extent::new(128, 128, 16), Spacing::new(0.4, 0.4, 1.2)),
        Level::new(Extent::new(64, 64, 8), Spacing::new(0.8, 0.8, 2.4)),
        Level::new(Extent::new(32, 32, 4), Spacing::new(1.6, 1.6, 4.8)),
    ];
    Pyramid::new(levels, channels(), frames, 1.0)
}

fn channels() -> Vec<ChannelInfo> {
    (0..2)
        .map(|channel| {
            ChannelInfo::new(
                format!("Channel {channel}"),
                [1.0, 1.0, 1.0],
                intensity(),
                intensity(),
                VoxelFormat::U16,
            )
        })
        .collect()
}

fn intensity() -> Intensity {
    Intensity::new(0.0, RAMP)
}

fn volume(extent: Extent, value: impl Fn(u32, u32, u32) -> f32) -> VoxelData {
    let mut voxels = Vec::with_capacity(extent.voxel_count() as usize);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                voxels.push((value(x, y, z) * RAMP) as u16);
            }
        }
    }
    VoxelData::U16(voxels)
}

fn slice(data: &VoxelData, extent: Extent, ranges: &[std::ops::Range<u32>; 3]) -> VoxelData {
    data.region(
        extent,
        ranges[0].clone(),
        ranges[1].clone(),
        ranges[2].clone(),
    )
}

fn inside(x: u32, y: u32, z: u32) -> bool {
    let point = [
        (x as f32 + 0.5) * 0.1 - 0.5 * EXTENT.x() as f32 * 0.1,
        (y as f32 + 0.5) * 0.1 - 0.5 * EXTENT.y() as f32 * 0.1,
        (z as f32 + 0.5) * 0.3 - 0.5 * EXTENT.z() as f32 * 0.3,
    ];
    point.iter().map(|value| value * value).sum::<f32>() <= 1.5 * 1.5
}

fn core(x: u32, y: u32) -> bool {
    x >= EXTENT.x() / 4 && x < 3 * EXTENT.x() / 4 && y >= EXTENT.y() / 4 && y < 3 * EXTENT.y() / 4
}

fn hash(x: u32, y: u32, z: u32, seed: u32) -> f32 {
    let mut value = x.wrapping_mul(73_856_093)
        ^ y.wrapping_mul(19_349_663)
        ^ z.wrapping_mul(83_492_791)
        ^ seed.wrapping_mul(2_654_435_761);
    value ^= value >> 13;
    value = value.wrapping_mul(0x85eb_ca6b);
    value ^= value >> 16;
    (value % 10_000) as f32 / 9_999.0
}
