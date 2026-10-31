use std::ops::Range;

use grana_object::{COLOC_BINS, COLOC_SCATTER, ColocFrame, ColocPair, ColocSeries};
use grana_volume::{Intensity, Level, Region, VoxelData};

const TILE: [u32; 3] = [256, 256, 32];
const SCATTER_SHIFT: usize = COLOC_BINS / COLOC_SCATTER;

pub struct ColocScan {
    pair: ColocPair,
    ranges: [Range<u32>; 3],
    counts: [usize; 3],
    scales: [[f32; 2]; 2],
    frames: Vec<ColocFrame>,
    counts_a: Vec<u64>,
    counts_b: Vec<u64>,
    scatter: Vec<u64>,
}

impl ColocScan {
    pub fn new(
        level: Level,
        frames: u32,
        channels: usize,
        intensities: [Intensity; 2],
        pair: ColocPair,
        region: Region,
    ) -> Self {
        assert!(
            pair.a() < channels && pair.b() < channels,
            "a colocalization pair names two channels of the dataset"
        );
        assert!(frames > 0, "a colocalization scan covers a frame");
        let Some(ranges) = region.voxel_range(level) else {
            panic!("the colocalization region overlaps the dataset");
        };
        let counts = [0, 1, 2].map(|axis| {
            let (start, end) = (ranges[axis].start, ranges[axis].end);
            ((end - start).div_ceil(TILE[axis])) as usize
        });
        let scales = intensities.map(|intensity| {
            let span = intensity.span();
            if span > 0.0 {
                [intensity.min(), 1.0 / span]
            } else {
                [intensity.min(), 0.0]
            }
        });
        Self {
            pair,
            ranges,
            counts,
            scales,
            frames: (0..frames).map(|_| ColocFrame::new()).collect(),
            counts_a: vec![0; COLOC_BINS],
            counts_b: vec![0; COLOC_BINS],
            scatter: vec![0; COLOC_SCATTER * COLOC_SCATTER],
        }
    }

    pub fn pair(&self) -> ColocPair {
        self.pair
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

    pub fn add(&mut self, time: u32, index: usize, a: &VoxelData, b: &VoxelData) {
        let tile = self.tile(index);
        let voxels = [0, 1, 2]
            .map(|axis| (tile[axis].end - tile[axis].start) as usize)
            .iter()
            .product::<usize>();
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "a colocalization tile carries both channels voxel for voxel"
        );
        let frame = &mut self.frames[time as usize];
        for voxel in 0..voxels {
            let a = unit(self.scales[0], a.value(voxel));
            let b = unit(self.scales[1], b.value(voxel));
            let bin_a = bin(a);
            let bin_b = bin(b);
            frame.push(a, b);
            self.counts_a[bin_a] += 1;
            self.counts_b[bin_b] += 1;
            self.scatter[(bin_a / SCATTER_SHIFT) + COLOC_SCATTER * (bin_b / SCATTER_SHIFT)] += 1;
        }
    }

    pub fn finish(self) -> ColocSeries {
        ColocSeries::new(
            self.pair,
            self.frames,
            self.counts_a,
            self.counts_b,
            self.scatter,
        )
    }
}

fn unit(scale: [f32; 2], value: f32) -> f32 {
    if value.is_finite() {
        ((value - scale[0]) * scale[1]).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn bin(unit: f32) -> usize {
    ((unit * COLOC_BINS as f32) as usize).min(COLOC_BINS - 1)
}
