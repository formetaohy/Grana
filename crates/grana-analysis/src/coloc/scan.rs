use std::ops::Range;

use grana_measure::{
    COLOC_BINS_MAX, COLOC_BINS_MIN, ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo,
    ColocSignificance,
};
use grana_volume::{Intensity, Level, Region, VoxelData};

use crate::mask::Mask;

use super::{ColocLevels, scale, unit};

const TILE: [u32; 3] = [256, 256, 32];
const WORKING_BINS: usize = COLOC_BINS_MAX;

pub struct ColocScanConfig {
    pair: ColocPair,
    intensities: [Intensity; 2],
    mask: Option<u64>,
    frames: u32,
}

impl ColocScanConfig {
    pub fn new(
        pair: ColocPair,
        channels: usize,
        intensities: [Intensity; 2],
        mask: Option<u64>,
        frames: u32,
    ) -> Self {
        assert!(
            pair.a() < channels && pair.b() < channels,
            "a colocalization pair names two channels of the dataset"
        );
        assert!(frames > 0, "a colocalization scan covers a frame");
        assert!(
            intensities.iter().all(|intensity| intensity.span() > 0.0),
            "a colocalization scan selects channels with an intensity range"
        );
        Self {
            pair,
            intensities,
            mask,
            frames,
        }
    }
}

pub struct ColocScan {
    pair: ColocPair,
    level: usize,
    permutation_level: usize,
    permutation_stride: u32,
    mask: Option<u64>,
    ranges: [Range<u32>; 3],
    counts: [usize; 3],
    scales: [[f32; 2]; 2],
    frames: Vec<ColocFrame>,
    joint: Vec<u64>,
}

impl ColocScan {
    pub fn new(info: Level, levels: &ColocLevels, config: ColocScanConfig, region: Region) -> Self {
        let ColocScanConfig {
            pair,
            intensities,
            mask,
            frames,
        } = config;
        let Some(ranges) = region.voxel_range(info) else {
            panic!("the colocalization region overlaps the dataset");
        };
        let counts = [0, 1, 2].map(|axis| {
            let (start, end) = (ranges[axis].start, ranges[axis].end);
            ((end - start).div_ceil(TILE[axis])) as usize
        });
        Self {
            pair,
            level: levels.scan(),
            permutation_level: levels.permutation(),
            permutation_stride: levels.stride(),
            mask,
            ranges,
            counts,
            scales: intensities.map(scale),
            frames: (0..frames).map(|_| ColocFrame::new(WORKING_BINS)).collect(),
            joint: vec![0; WORKING_BINS * WORKING_BINS],
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn voxels(&self) -> u64 {
        self.frames
            .iter()
            .map(|frame| frame.moments().voxels())
            .sum()
    }

    pub fn tile_count(&self) -> usize {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    pub fn tile(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.tile_count(), "the tile index lies on the grid");
        let x = index % self.counts[0];
        let y = (index / self.counts[0]) % self.counts[1];
        let z = index / (self.counts[0] * self.counts[1]);
        [0, 1, 2].map(|axis| {
            let offset = [x, y, z][axis] as u32 * TILE[axis];
            let start = self.ranges[axis].start + offset;
            start..(start + TILE[axis]).min(self.ranges[axis].end)
        })
    }

    pub fn add(
        &mut self,
        time: u32,
        index: usize,
        a: &VoxelData,
        b: &VoxelData,
        mask: Option<&Mask>,
    ) {
        let tile = self.tile(index);
        let dims = [0, 1, 2].map(|axis| tile[axis].end - tile[axis].start);
        let voxels = dims.iter().map(|dim| *dim as usize).product::<usize>();
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "a colocalization tile carries both channels voxel for voxel"
        );
        let scales = self.scales;
        let frame = &mut self.frames[time as usize];
        for z in 0..dims[2] {
            let global_z = tile[2].start + z;
            for y in 0..dims[1] {
                let global_y = tile[1].start + y;
                for x in 0..dims[0] {
                    let global_x = tile[0].start + x;
                    if let Some(mask) = mask
                        && !mask.contains_cell([global_x, global_y, global_z])
                    {
                        continue;
                    }
                    let voxel = x as usize
                        + dims[0] as usize * (y as usize + dims[1] as usize * z as usize);
                    let first = unit(scales[0], a.value(voxel));
                    let second = unit(scales[1], b.value(voxel));
                    let bin_a = working_bin(first);
                    let bin_b = working_bin(second);
                    frame.push(first, second);
                    self.joint[bin_a + WORKING_BINS * bin_b] += 1;
                }
            }
        }
    }

    pub fn finish(
        self,
        significance: Option<ColocSignificance>,
        permutation_voxels: u64,
        permutation_frames: u32,
        randomizations: u32,
        blocks: u64,
    ) -> ColocSeries {
        let mut working = ColocFrame::new(WORKING_BINS);
        for frame in &self.frames {
            working.merge(frame);
        }
        let (counts_a, counts_b) = marginals(&self.joint);
        let [sum_a, sum_b, sum_aa, sum_bb, ..] = working.moments().sums();
        let voxels = working.moments().voxels();
        let bins = bins_for(&counts_a, sum_a, sum_aa, voxels)
            .max(bins_for(&counts_b, sum_b, sum_bb, voxels));
        let frames = self
            .frames
            .iter()
            .map(|frame| aggregate(frame, bins))
            .collect::<Vec<_>>();
        let joint = aggregate_joint(&self.joint, bins);
        let cost = ColocCost::new(
            voxels,
            permutation_voxels,
            permutation_frames,
            randomizations,
            blocks,
        );
        let info = ColocSeriesInfo::new(
            self.level,
            self.permutation_level,
            self.permutation_stride,
            self.mask,
        );
        ColocSeries::from_parts(self.pair, info, bins, frames, joint, significance, cost)
    }
}

fn working_bin(unit: f32) -> usize {
    ((unit * WORKING_BINS as f32) as usize).min(WORKING_BINS - 1)
}

fn marginals(joint: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let mut counts_a = vec![0; WORKING_BINS];
    let mut counts_b = vec![0; WORKING_BINS];
    for b in 0..WORKING_BINS {
        for a in 0..WORKING_BINS {
            let count = joint[a + WORKING_BINS * b];
            counts_a[a] += count;
            counts_b[b] += count;
        }
    }
    (counts_a, counts_b)
}

fn aggregate(frame: &ColocFrame, bins: usize) -> ColocFrame {
    let mut a_by_b = vec![0.0; bins];
    let mut b_by_a = vec![0.0; bins];
    for (index, value) in frame.a_by_b().iter().enumerate() {
        a_by_b[index * bins / WORKING_BINS] += value;
    }
    for (index, value) in frame.b_by_a().iter().enumerate() {
        b_by_a[index * bins / WORKING_BINS] += value;
    }
    ColocFrame::from_parts(frame.moments(), a_by_b, b_by_a)
}

fn aggregate_joint(joint: &[u64], bins: usize) -> Vec<u64> {
    let mut aggregated = vec![0; bins * bins];
    for b in 0..WORKING_BINS {
        for a in 0..WORKING_BINS {
            let count = joint[a + WORKING_BINS * b];
            if count == 0 {
                continue;
            }
            aggregated[a * bins / WORKING_BINS + bins * (b * bins / WORKING_BINS)] += count;
        }
    }
    aggregated
}

fn bins_for(counts: &[u64], sum: f64, squares: f64, voxels: u64) -> usize {
    let voxels = voxels as f64;
    let mean = sum / voxels;
    let deviation = (squares / voxels - mean * mean).max(0.0).sqrt();
    let spread = interquartile(counts);
    let scale = voxels.powf(-1.0 / 3.0);
    let width = if spread > 0.0 {
        2.0 * spread * scale
    } else if deviation > 0.0 {
        3.49 * deviation * scale
    } else {
        return COLOC_BINS_MIN;
    };
    ((1.0 / width).ceil() as usize).clamp(COLOC_BINS_MIN, COLOC_BINS_MAX)
}

fn interquartile(counts: &[u64]) -> f64 {
    let total = counts.iter().sum::<u64>();
    if total == 0 {
        return 0.0;
    }
    let quantile = |fraction: f64| {
        let position = fraction * total as f64;
        let mut cumulative = 0.0;
        for (index, count) in counts.iter().enumerate() {
            let next = cumulative + *count as f64;
            if position <= next && *count > 0 {
                let within = ((position - cumulative) / *count as f64).clamp(0.0, 1.0);
                return (index as f64 + within) / counts.len() as f64;
            }
            cumulative = next;
        }
        1.0
    };
    quantile(0.75) - quantile(0.25)
}
