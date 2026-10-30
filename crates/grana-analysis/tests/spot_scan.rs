use grana_analysis::{SpotParams, SpotScan};
use grana_object::Spot;
use grana_volume::{Extent, Level, Spacing, VoxelData};

const EXTENT: Extent = Extent::new(200, 96, 40);
const DIAMETER: f32 = 0.4;
const AMPLITUDE: f32 = 12_000.0;
const BACKGROUND: f32 = 200.0;

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
    let mut scan = SpotScan::new(level, span, SpotParams::new(DIAMETER, 0.2));
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

    let mut strict = SpotScan::new(level, span, SpotParams::new(DIAMETER, 1.0));
    assert!(scan_all(&mut strict, &data, level).is_empty());
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
        spots.extend(scan.detect(index, &values));
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
