use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::{SpotScan, SpotScanner};
use grana_object::SpotParams;
use grana_object::{Spot, SpotSet};
use grana_volume::{Extent, Level, Region, Spacing, VoxelData};

const EXTENT: Extent = Extent::new(200, 96, 40);
const DIAMETER: f32 = 0.4;
const AMPLITUDE: f32 = 12_000.0;
const BACKGROUND: f32 = 200.0;
const MARKER: u16 = 981;

fn spacing() -> Spacing {
    Spacing::new(0.1, 0.1, 0.3)
}

struct Blob {
    center: [f32; 3],
    world: [f32; 3],
}

#[test]
fn detects_blobs_across_blocks() {
    let blobs = [
        Blob {
            center: [100.0, 50.0, 20.0],
            world: [0.05, 0.25, 0.15],
        },
        Blob {
            center: [64.3, 30.0, 12.0],
            world: [-3.52, -1.75, -2.25],
        },
    ];
    let data = volume(&blobs);
    let level = Level::new(EXTENT, spacing());
    let span = data.range().span();
    let mut scan = SpotScan::new(level, span, SpotParams::new(DIAMETER, 0.2), Region::none());
    let spots = scan_all(&mut scan, &data, level);

    assert_eq!(spots.len(), blobs.len(), "every blob is detected once");
    for blob in &blobs {
        let nearest = spots
            .iter()
            .map(|spot| {
                let center = spot.center();
                let distance = (0..3)
                    .map(|axis| (center[axis] - blob.world[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                (distance, spot.quality())
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .expect("a spot was detected");
        assert!(
            nearest.0 < 0.2,
            "the blob at {:?} was localised {:.3} away",
            blob.world,
            nearest.0
        );
        assert!(
            nearest.1 > 0.4,
            "the blob at {:?} has a weak contrast {}",
            blob.world,
            nearest.1
        );
    }

    let mut strict = SpotScan::new(level, span, SpotParams::new(DIAMETER, 1.0), Region::none());
    assert!(scan_all(&mut strict, &data, level).is_empty());

    let region = Region::new(Vec3::new(-1.0, -6.0, -6.0), Vec3::new(2.0, 6.0, 6.0));
    let mut restricted = SpotScan::new(level, span, SpotParams::new(DIAMETER, 0.2), region);
    let inside = scan_all(&mut restricted, &data, level);
    assert_eq!(inside.len(), 1, "the region holds a single blob");
    assert!(
        (inside[0].center()[0] - blobs[0].world[0]).abs() < 0.2,
        "the region keeps the blob it covers"
    );
}

#[test]
fn measures_spot_intensity() {
    let level = Level::new(EXTENT, spacing());
    let scan = SpotScan::new(
        level,
        1000.0,
        SpotParams::new(DIAMETER, 0.2),
        Region::none(),
    );
    let range = scan.read_range(0);
    let dims = [
        (range[0].end - range[0].start) as usize,
        (range[1].end - range[1].start) as usize,
        (range[2].end - range[2].start) as usize,
    ];
    let values = vec![500.0f32; dims[0] * dims[1] * dims[2]];
    let world = level.world_extent();
    let spacing = spacing();
    let axis_spacing = [spacing.x(), spacing.y(), spacing.z()];
    let voxel = [
        range[0].start as f32 + 0.5 * dims[0] as f32,
        range[1].start as f32 + 0.5 * dims[1] as f32,
        range[2].start as f32 + 0.5 * dims[2] as f32,
    ];
    let center = [0, 1, 2].map(|axis| (voxel[axis] + 0.5) * axis_spacing[axis] - 0.5 * world[axis]);
    let spot = Spot::new(center, 0.5, 1.0);
    let statistics = scan.measure(0, &[spot], &values)[0].statistics();
    assert_eq!(statistics.mean(), 500.0);
    assert_eq!(statistics.minimum(), 500.0);
    assert_eq!(statistics.maximum(), 500.0);
    assert_eq!(statistics.deviation(), 0.0);
    assert!(statistics.sum() >= 500.0, "the sphere samples voxels");

    let small = Spot::new(center, 1.0e-3, 1.0);
    let statistics = scan.measure(0, &[small], &values)[0].statistics();
    assert_eq!(
        statistics.sum(),
        500.0,
        "a sub voxel spot measures its containing voxel"
    );
}

fn scan_all(scan: &mut SpotScan, data: &VoxelData, level: Level) -> Vec<Spot> {
    let mut spots = Vec::new();
    let mut values = Vec::new();
    let extent = level.extent();
    for index in 0..scan.block_count() {
        let range = scan.read_range(index);
        let region = data.region(extent, range[0].clone(), range[1].clone(), range[2].clone());
        values.clear();
        values.resize(region.voxel_count(), 0.0);
        region.write_values(&mut values);
        spots.extend(scan.detect(index, &values, None));
    }
    spots
}

fn volume(blobs: &[Blob]) -> VoxelData {
    let sigma = DIAMETER / 2.354_82;
    let mut values = vec![BACKGROUND; EXTENT.voxel_count() as usize];
    for blob in blobs {
        for z in 0..EXTENT.z() {
            for y in 0..EXTENT.y() {
                for x in 0..EXTENT.x() {
                    let offset = [
                        (x as f32 - blob.center[0]) * spacing().x(),
                        (y as f32 - blob.center[1]) * spacing().y(),
                        (z as f32 - blob.center[2]) * spacing().z(),
                    ];
                    let squared = offset.iter().map(|value| value * value).sum::<f32>();
                    let value = AMPLITUDE * (-squared / (2.0 * sigma * sigma)).exp();
                    let index = EXTENT.index(x, y, z) as usize;
                    values[index] = (values[index] + value).min(f32::from(u16::MAX));
                }
            }
        }
    }
    VoxelData::U16(values.into_iter().map(|value| value as u16).collect())
}

#[test]
fn keeps_spots_deterministic_across_worker_counts() {
    let blobs = [
        Blob {
            center: [100.0, 50.0, 20.0],
            world: [0.05, 0.25, 0.15],
        },
        Blob {
            center: [64.3, 30.0, 12.0],
            world: [-3.52, -1.75, -2.25],
        },
    ];
    let data = volume(&blobs);
    let level = Level::new(EXTENT, spacing());
    let span = data.range().span();
    let single = scan_frame(level, span, &data, 1);
    let many = scan_frame(level, span, &data, 8);
    assert_eq!(single.spots(), many.spots(), "the spots are identical");
    assert_eq!(
        single.intensities(),
        many.intensities(),
        "the measured intensities are identical"
    );
    assert_eq!(
        single.intensities().object(0, 1).mean(),
        f32::from(MARKER),
        "the second channel is measured inside the spots"
    );
}

fn scan_frame(level: Level, span: f32, data: &VoxelData, workers: usize) -> SpotSet {
    let scanner = SpotScanner::new(
        0,
        level,
        span,
        SpotParams::new(DIAMETER, 0.2),
        Region::none(),
        2,
        0,
    );
    let cancel = AtomicBool::new(false);
    scanner
        .frame(
            workers,
            &cancel,
            None,
            &|channel, range| {
                let region = data.region(
                    level.extent(),
                    range[0].clone(),
                    range[1].clone(),
                    range[2].clone(),
                );
                Ok(if channel == 0 {
                    region
                } else {
                    VoxelData::U16(vec![MARKER; region.voxel_count()])
                })
            },
            &mut |_done, _total| {},
        )
        .expect("the frame scans")
        .expect("the spot scan completes")
}
