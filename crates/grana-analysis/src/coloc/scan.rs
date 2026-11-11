use std::sync::atomic::AtomicBool;

use grana_measure::{
    COLOC_BINS_MAX, COLOC_BINS_MIN, ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo,
    ColocSignificance,
};
use grana_volume::{Intensity, Level, Region, VoxelData};

use crate::schedule;
use grana_object::Mask;

use super::sampling::{ColocSampling, dims_product};
use super::test;
use super::{ColocRead, scale, unit};

const WORKING_BINS: usize = COLOC_BINS_MAX;

pub struct ColocScanConfig {
    pair: ColocPair,
    intensities: [Intensity; 2],
    mask: Option<u64>,
}

impl ColocScanConfig {
    pub fn new(
        pair: ColocPair,
        channels: usize,
        intensities: [Intensity; 2],
        mask: Option<u64>,
    ) -> Self {
        assert!(
            pair.a() < channels && pair.b() < channels,
            "a colocalization pair names two channels of the dataset"
        );
        assert!(
            intensities.iter().all(|intensity| intensity.span() > 0.0),
            "a colocalization scan selects channels with an intensity range"
        );
        Self {
            pair,
            intensities,
            mask,
        }
    }
}

pub(super) struct FrameSample {
    pub a: Vec<f32>,
    pub b: Vec<f32>,
    pub included: Vec<u32>,
}

pub struct ColocScan {
    pair: ColocPair,
    sampling: ColocSampling,
    mask: Option<u64>,
    region_voxels: u64,
    scales: [[f32; 2]; 2],
    frames: Vec<ColocFrame>,
    joint: Vec<u64>,
    sample: Vec<FrameSample>,
}

impl ColocScan {
    pub fn new(
        info: Level,
        sampling: ColocSampling,
        config: ColocScanConfig,
        region: Region,
    ) -> Self {
        let ColocScanConfig {
            pair,
            intensities,
            mask,
        } = config;
        let Some(ranges) = region.voxel_range(info) else {
            panic!("the colocalization region overlaps the dataset");
        };
        let region_voxels = ranges
            .iter()
            .map(|range| u64::from(range.end - range.start))
            .product();
        let frames = sampling.frames() as usize;
        let voxels = sampling.voxels();
        Self {
            pair,
            sampling,
            mask,
            region_voxels,
            scales: intensities.map(scale),
            frames: (0..frames).map(|_| ColocFrame::new(WORKING_BINS)).collect(),
            joint: vec![0; WORKING_BINS * WORKING_BINS],
            sample: (0..frames)
                .map(|_| FrameSample {
                    a: vec![0.0; voxels],
                    b: vec![0.0; voxels],
                    included: Vec::with_capacity(voxels),
                })
                .collect(),
        }
    }

    pub fn level(&self) -> usize {
        self.sampling.level()
    }

    pub fn stride(&self) -> u32 {
        self.sampling.stride()
    }

    pub fn voxels(&self) -> u64 {
        self.frames
            .iter()
            .map(|frame| frame.moments().voxels())
            .sum()
    }

    pub fn tile_count(&self) -> usize {
        self.sampling.grid().tile_count()
    }

    pub fn tile(&self, index: usize) -> [std::ops::Range<u32>; 3] {
        self.sampling.grid().tile(index)
    }

    pub fn scan(
        &mut self,
        workers: usize,
        cancel: &AtomicBool,
        read: &ColocRead<'_>,
        mask: &(dyn Fn(u32) -> Result<Option<Mask>, String> + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<bool, String> {
        let tiles = self.tile_count();
        let total = self.frames.len() * tiles;
        let sampling = &self.sampling;
        let pair = self.pair;
        let scales = self.scales;
        let frames = &mut self.frames;
        let joint = &mut self.joint;
        let sample = &mut self.sample;
        let completed = schedule::ordered(
            schedule::Run {
                total,
                workers: schedule::workers(workers),
                cancel,
                progress,
            },
            ColocWorker::new,
            |worker, index| {
                let time = index / tiles;
                let time = u32::try_from(time).expect("a colocalization frame fits the u32 range");
                if worker
                    .mask
                    .as_ref()
                    .is_none_or(|(cached, _)| *cached != time)
                {
                    worker.mask = Some((time, mask(time)?));
                }
                let cached = worker.mask.as_ref().and_then(|(_, mask)| mask.clone());
                let ranges = sampling.grid().tile(index % tiles);
                let a = read(time, pair.a(), &ranges)?;
                let b = read(time, pair.b(), &ranges)?;
                Ok(worker.tile(
                    sampling,
                    scales,
                    TileRead {
                        time,
                        ranges,
                        a: &a,
                        b: &b,
                        mask: cached.as_ref(),
                    },
                ))
            },
            |index, tile| {
                assert_eq!(
                    index / tiles,
                    tile.time as usize,
                    "a colocalization tile lands on its frame"
                );
                let slot = tile.time as usize;
                frames[slot].merge(&tile.frame);
                let sample = &mut sample[slot];
                for (position, compact) in tile.indices.iter().enumerate() {
                    let compact = *compact as usize;
                    sample.a[compact] = tile.a[position];
                    sample.b[compact] = tile.b[position];
                }
                sample.included.extend_from_slice(&tile.included);
                Ok(())
            },
            |worker| {
                for (slot, count) in worker.joint.into_iter().enumerate() {
                    joint[slot] += count;
                }
            },
        )?;
        Ok(completed)
    }

    pub fn test(
        &self,
        randomizations: u32,
        workers: usize,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Option<ColocSignificance> {
        test::permutation(
            test::Input {
                frames: &self.sample,
                dims: self.sampling.dims(),
                block: self.sampling.block(),
                blocks: self.sampling.blocks(),
                randomizations,
                seed: seed(self.pair, self.sampling.level(), self.sampling.stride()),
                workers,
                cancel,
            },
            progress,
        )
    }

    pub fn finish(
        self,
        significance: Option<ColocSignificance>,
        randomizations: u32,
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
        let cost = ColocCost::new(voxels, randomizations, self.sampling.blocks());
        let coverage = (voxels as f64 / self.region_voxels as f64).clamp(0.0, 1.0) as f32;
        let info = ColocSeriesInfo::new(
            self.sampling.level(),
            self.sampling.stride(),
            self.mask,
            coverage,
        );
        ColocSeries::from_parts(self.pair, info, bins, frames, joint, significance, cost)
    }
}

fn seed(pair: ColocPair, level: usize, stride: u32) -> u64 {
    test::SEED
        ^ ((pair.a() as u64) << 40)
        ^ ((pair.b() as u64) << 20)
        ^ ((level as u64) << 8)
        ^ u64::from(stride)
}

struct ColocWorker {
    joint: Vec<u64>,
    mask: Option<(u32, Option<Mask>)>,
    indices: Vec<u32>,
    included: Vec<u32>,
    a: Vec<f32>,
    b: Vec<f32>,
}

struct ColocTile {
    time: u32,
    frame: ColocFrame,
    indices: Vec<u32>,
    included: Vec<u32>,
    a: Vec<f32>,
    b: Vec<f32>,
}

struct TileRead<'a> {
    time: u32,
    ranges: [std::ops::Range<u32>; 3],
    a: &'a VoxelData,
    b: &'a VoxelData,
    mask: Option<&'a Mask>,
}

impl ColocWorker {
    fn new() -> Self {
        Self {
            joint: vec![0; WORKING_BINS * WORKING_BINS],
            mask: None,
            indices: Vec::new(),
            included: Vec::new(),
            a: Vec::new(),
            b: Vec::new(),
        }
    }

    fn tile(
        &mut self,
        sampling: &ColocSampling,
        scales: [[f32; 2]; 2],
        read: TileRead<'_>,
    ) -> ColocTile {
        let TileRead {
            time,
            ranges: tile,
            a,
            b,
            mask,
        } = read;
        let dims = [0, 1, 2].map(|axis| tile[axis].end - tile[axis].start);
        let voxels = dims_product(dims);
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "a colocalization tile carries both channels voxel for voxel"
        );
        self.indices.clear();
        self.included.clear();
        self.a.clear();
        self.b.clear();
        let mut frame = ColocFrame::new(WORKING_BINS);
        let stride = sampling.stride() as usize;
        let compact_dims = sampling.dims();
        let ranges = sampling.grid().ranges();
        let mut offsets = [0usize; 3];
        let mut starts = [0usize; 3];
        for axis in 0..3 {
            let relative = (tile[axis].start - ranges[axis].start) as usize;
            offsets[axis] = (stride - relative % stride) % stride;
            starts[axis] = (relative + offsets[axis]) / stride;
        }
        let mut z = offsets[2];
        let mut compact_z = starts[2];
        while z < dims[2] as usize {
            let mut y = offsets[1];
            let mut compact_y = starts[1];
            while y < dims[1] as usize {
                let mut x = offsets[0];
                let mut compact_x = starts[0];
                while x < dims[0] as usize {
                    let global = [
                        tile[0].start + x as u32,
                        tile[1].start + y as u32,
                        tile[2].start + z as u32,
                    ];
                    let voxel = x + dims[0] as usize * (y + dims[1] as usize * z);
                    let compact = compact_x
                        + compact_dims[0] as usize
                            * (compact_y + compact_dims[1] as usize * compact_z);
                    let first = unit(scales[0], a.value(voxel));
                    let second = unit(scales[1], b.value(voxel));
                    let compact = u32::try_from(compact)
                        .expect("a colocalization sample grid fits the u32 index");
                    self.indices.push(compact);
                    self.a.push(first);
                    self.b.push(second);
                    if mask.is_none_or(|mask| mask.contains_cell(global)) {
                        frame.push(first, second);
                        let bin = working_bin(first) + WORKING_BINS * working_bin(second);
                        self.joint[bin] += 1;
                        self.included.push(compact);
                    }
                    x += stride;
                    compact_x += 1;
                }
                y += stride;
                compact_y += 1;
            }
            z += stride;
            compact_z += 1;
        }
        ColocTile {
            time,
            frame,
            indices: std::mem::take(&mut self.indices),
            included: std::mem::take(&mut self.included),
            a: std::mem::take(&mut self.a),
            b: std::mem::take(&mut self.b),
        }
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
