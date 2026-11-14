use grana_measure::{COLOC_BLOCK_MAX, ColocPrecision};
use grana_volume::{Level, Pyramid, Region};

use super::grid::Grid;

const BALANCED_VOXELS: u64 = 1 << 22;
const FAST_VOXELS: u64 = 1 << 20;
const SAMPLE_VOXELS: u64 = 1 << 19;

pub struct ColocSampling {
    level: usize,
    stride: u32,
    grid: Grid,
    dims: [u32; 3],
    block: [u32; 3],
    frames: u32,
}

impl ColocSampling {
    pub fn level(&self) -> usize {
        self.level
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub(super) fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    pub fn block(&self) -> [u32; 3] {
        self.block
    }

    pub fn voxels(&self) -> usize {
        dims_product(self.dims)
    }

    pub fn blocks(&self) -> u64 {
        [0, 1, 2]
            .map(|axis| u64::from(self.dims[axis].div_ceil(self.block[axis])))
            .into_iter()
            .product()
    }
}

pub fn coloc_sampling(
    pyramid: &Pyramid,
    region: Region,
    precision: ColocPrecision,
    block: u32,
) -> ColocSampling {
    assert!(
        (1..=COLOC_BLOCK_MAX).contains(&block),
        "a colocalization randomization block spans a supported number of voxels"
    );
    let frames = pyramid.frame_count();
    assert!(frames > 0, "a colocalization sample covers a frame");
    let levels = (0..pyramid.level_count())
        .filter_map(|index| voxels(region, pyramid.level(index)).map(|voxels| (index, voxels)))
        .collect::<Vec<_>>();
    assert!(
        !levels.is_empty(),
        "the colocalization region overlaps the dataset"
    );
    let budget = match precision {
        ColocPrecision::Full => u64::MAX,
        ColocPrecision::Balanced => BALANCED_VOXELS,
        ColocPrecision::Fast => FAST_VOXELS,
    };
    let level = levels
        .iter()
        .find(|(_, count)| *count <= budget)
        .map_or_else(
            || levels.last().expect("the sampling level exists").0,
            |(index, _)| *index,
        );
    let info = pyramid.level(level);
    let ranges = region
        .voxel_range(info)
        .expect("the sampling level carries the colocalization region");
    let lengths = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
    let limit = lengths.iter().copied().max().unwrap_or(1).max(1);
    let mut stride = 1u32;
    while stride < limit
        && sampled(&lengths, stride).saturating_mul(u64::from(frames)) > SAMPLE_VOXELS
    {
        stride += 1;
    }
    let dims = lengths.map(|length| length.div_ceil(stride));
    let block = block_size(block, info, pyramid.level(0), stride, dims);
    ColocSampling {
        level,
        stride,
        grid: Grid::new(ranges),
        dims,
        block,
        frames,
    }
}

pub(super) fn dims_product(dims: [u32; 3]) -> usize {
    dims.iter().map(|dim| *dim as usize).product()
}

fn sampled(lengths: &[u32; 3], stride: u32) -> u64 {
    lengths
        .iter()
        .map(|length| u64::from(length.div_ceil(stride)))
        .product()
}

fn voxels(region: Region, level: Level) -> Option<u64> {
    region.voxel_range(level).map(|ranges| {
        ranges
            .iter()
            .map(|range| u64::from(range.end - range.start))
            .product()
    })
}

fn block_size(block: u32, info: Level, base: Level, stride: u32, dims: [u32; 3]) -> [u32; 3] {
    let spacing = info.spacing();
    let base_spacing = base.spacing();
    [
        (spacing.x(), base_spacing.x()),
        (spacing.y(), base_spacing.y()),
        (spacing.z(), base_spacing.z()),
    ]
    .into_iter()
    .zip(dims)
    .map(|((spacing, base_spacing), dim)| {
        let ratio = spacing / base_spacing;
        let size = (block as f32 / (ratio * stride as f32)).round().max(1.0) as u32;
        size.min(dim)
    })
    .collect::<Vec<u32>>()
    .try_into()
    .expect("a colocalization block spans three axes")
}
