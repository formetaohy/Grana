mod grid;
mod scan;
mod significance;

use std::ops::Range;

use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

pub use scan::{ColocScan, ColocScanConfig};
pub use significance::ColocPermutation;

pub type ColocRead<'a> =
    dyn Fn(u32, usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

const SCAN_VOXELS: u64 = 1 << 22;
const PERMUTATION_VOXELS: u64 = 1 << 17;
const PERMUTATION_SAMPLES: u64 = 1 << 19;
const PERMUTATION_FLOOR: u64 = 1 << 15;
const PERMUTATION_FRAMES: u32 = 8;
const PERMUTATION_DEPTH: usize = 2;

pub struct ColocLevels {
    scan: usize,
    permutation: usize,
    stride: u32,
    frames: Vec<u32>,
}

impl ColocLevels {
    pub fn scan(&self) -> usize {
        self.scan
    }

    pub fn permutation(&self) -> usize {
        self.permutation
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn frames(&self) -> &[u32] {
        &self.frames
    }
}

pub fn coloc_levels(pyramid: &Pyramid, region: Region, frames: u32) -> ColocLevels {
    assert!(frames > 0, "a colocalization scan covers a frame");
    let levels = (0..pyramid.level_count())
        .filter_map(|index| voxels(region, pyramid.level(index)).map(|voxels| (index, voxels)))
        .collect::<Vec<_>>();
    assert!(
        !levels.is_empty(),
        "the colocalization region overlaps the dataset"
    );
    let scan = levels
        .iter()
        .find(|(_, count)| *count <= SCAN_VOXELS)
        .map_or_else(
            || levels.last().expect("the scan level exists").0,
            |(index, _)| *index,
        );
    let limit = scan.saturating_add(PERMUTATION_DEPTH);
    let candidates = levels
        .iter()
        .copied()
        .filter(|(index, _)| *index >= scan && *index <= limit)
        .collect::<Vec<_>>();
    let mut permutation = candidates
        .iter()
        .filter(|(_, count)| *count <= PERMUTATION_VOXELS)
        .map(|(index, _)| *index)
        .max()
        .unwrap_or_else(|| candidates.last().expect("the permutation level exists").0);
    loop {
        let ranges = region
            .voxel_range(pyramid.level(permutation))
            .expect("the permutation level carries the colocalization region");
        let lengths = ranges.map(|range| range.end - range.start);
        let wanted = frames.min(PERMUTATION_FRAMES);
        let budget = (PERMUTATION_SAMPLES / u64::from(wanted)).max(1);
        let mut stride = 1u32;
        let mut sampled = lengths
            .iter()
            .map(|length| u64::from(*length))
            .product::<u64>();
        while sampled > budget {
            stride += 1;
            sampled = lengths
                .iter()
                .map(|length| u64::from(length.div_ceil(stride)))
                .product::<u64>();
        }
        let sampled_frames = (PERMUTATION_SAMPLES / sampled).clamp(1, u64::from(wanted)) as u32;
        if sampled * u64::from(sampled_frames) >= PERMUTATION_FLOOR || permutation == scan {
            return ColocLevels {
                scan,
                permutation,
                stride,
                frames: sample_frames(frames, sampled_frames),
            };
        }
        permutation -= 1;
    }
}

pub(crate) fn unit(scale: [f32; 2], value: f32) -> f32 {
    if value.is_finite() {
        ((value - scale[0]) * scale[1]).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(crate) fn scale(intensity: Intensity) -> [f32; 2] {
    let span = intensity.span();
    if span > 0.0 {
        [intensity.min(), 1.0 / span]
    } else {
        [intensity.min(), 0.0]
    }
}

fn voxels(region: Region, level: Level) -> Option<u64> {
    region.voxel_range(level).map(|ranges: [Range<u32>; 3]| {
        ranges
            .iter()
            .map(|range| u64::from(range.end - range.start))
            .product()
    })
}

fn sample_frames(frames: u32, count: u32) -> Vec<u32> {
    assert!(
        count > 0 && count <= frames,
        "a colocalization significance test samples frames of the dataset"
    );
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| ((u64::from(frames - 1) * u64::from(index)) / u64::from(count - 1)) as u32)
        .collect()
}
