use glam::Vec3;
use grana_analysis::ColocScan;
use grana_measure::{COLOC_BINS, COLOC_SCATTER, ColocPair, ColocSeries, ColocThresholds};
use grana_volume::{Extent, Intensity, Level, Region, Spacing, VoxelData};

const EXTENT: Extent = Extent::new(64, 48, 8);
const RAMP: f32 = 65535.0;

fn level() -> Level {
    Level::new(EXTENT, Spacing::new(0.1, 0.1, 0.3))
}

fn intensity() -> Intensity {
    Intensity::new(0.0, RAMP)
}

#[test]
fn measures_correlated_channels() {
    let first = ramp(|x, _, _| x as f32 / (EXTENT.x() - 1) as f32);
    let second = first.clone();
    let series = scan(Region::none(), &first, &second);
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
    let above = (0..EXTENT.x())
        .filter(|x| *x as f32 / (EXTENT.x() - 1) as f32 > 0.5)
        .map(|x| x as f64)
        .sum::<f64>();
    let total = (0..EXTENT.x()).map(|x| x as f64).sum::<f64>();
    assert!((manders[0].unwrap() - above / total).abs() < 1.0e-3);
    let otsu = series.otsu();
    assert!(
        (0.3..0.7).contains(&otsu.a()),
        "the otsu threshold splits the ramp near its middle"
    );
    assert_eq!(series.counts_a().iter().sum::<u64>(), EXTENT.voxel_count());
    assert_eq!(series.scatter().iter().sum::<u64>(), EXTENT.voxel_count());
    assert_eq!(series.counts_a().len(), COLOC_BINS);
    assert_eq!(series.scatter().len(), COLOC_SCATTER * COLOC_SCATTER);
}

#[test]
fn measures_anticorrelated_channels_inside_a_region() {
    let first = ramp(|x, _, _| x as f32 / (EXTENT.x() - 1) as f32);
    let second = ramp(|x, _, _| 1.0 - x as f32 / (EXTENT.x() - 1) as f32);
    let region = Region::new(Vec3::new(-1.0, -1.0, -2.0), Vec3::new(1.0, 1.0, 2.0));
    let series = scan(region, &first, &second);
    let statistics = series.statistics(ColocThresholds::new(0.5, 0.5));
    assert!(
        statistics.voxels() < EXTENT.voxel_count(),
        "the region carries fewer voxels than the volume"
    );
    assert!(
        statistics.pearson().is_some_and(|value| value < -0.999),
        "opposite ramps anticorrelate"
    );
}

fn scan(region: Region, first: &VoxelData, second: &VoxelData) -> ColocSeries {
    let mut scan = ColocScan::new(
        level(),
        1,
        2,
        [intensity(), intensity()],
        ColocPair::new(0, 1),
        region,
    );
    for index in 0..scan.tile_count() {
        let ranges = scan.tile(index);
        let a = slice(first, &ranges);
        let b = slice(second, &ranges);
        scan.add(0, index, &a, &b);
    }
    scan.finish()
}

fn ramp(value: impl Fn(u32, u32, u32) -> f32) -> VoxelData {
    let mut voxels = Vec::with_capacity(EXTENT.voxel_count() as usize);
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                voxels.push((value(x, y, z) * RAMP) as u16);
            }
        }
    }
    VoxelData::U16(voxels)
}

fn slice(data: &VoxelData, ranges: &[std::ops::Range<u32>; 3]) -> VoxelData {
    data.region(
        EXTENT,
        ranges[0].clone(),
        ranges[1].clone(),
        ranges[2].clone(),
    )
}
