use std::ops::Range;

use grana_measure::{ColocPair, ColocSettings, ColocSignificance};
use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

use crate::mask::Mask;

use super::{ColocLevels, scale, unit};

const TILE: [u32; 3] = [256, 256, 32];
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const EPSILON: f64 = 1.0e-12;
const MINIMUM: u64 = 3;

struct Frame {
    a: Vec<f32>,
    b: Vec<f32>,
    included: Vec<u32>,
}

pub struct ColocPermutation {
    pair: ColocPair,
    level: usize,
    stride: u32,
    settings: ColocSettings,
    ranges: [Range<u32>; 3],
    tiles: [usize; 3],
    dims: [u32; 3],
    block: [u32; 3],
    scales: [[f32; 2]; 2],
    frames: Vec<Frame>,
}

impl ColocPermutation {
    pub fn new(
        pyramid: &Pyramid,
        region: Region,
        pair: ColocPair,
        levels: &ColocLevels,
        settings: ColocSettings,
        intensities: [Intensity; 2],
    ) -> Self {
        let level = levels.permutation();
        let info = pyramid.level(level);
        let Some(ranges) = region.voxel_range(info) else {
            panic!("the permutation level carries the colocalization region");
        };
        let stride = levels.stride();
        let lengths = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
        let dims = lengths.map(|length| length.div_ceil(stride));
        let tiles = [0, 1, 2].map(|axis| lengths[axis].div_ceil(TILE[axis]) as usize);
        let block = block_size(settings.block(), info, pyramid.level(0), stride, dims);
        let voxels = dims.iter().map(|dim| *dim as usize).product::<usize>();
        Self {
            pair,
            level,
            stride,
            settings,
            ranges,
            tiles,
            dims,
            block,
            scales: intensities.map(scale),
            frames: (0..levels.frames().len())
                .map(|_| Frame {
                    a: vec![0.0; voxels],
                    b: vec![0.0; voxels],
                    included: Vec::with_capacity(voxels),
                })
                .collect(),
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tiles[0] * self.tiles[1] * self.tiles[2]
    }

    pub fn tile(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.tile_count(), "the tile index lies on the grid");
        let x = index % self.tiles[0];
        let y = (index / self.tiles[0]) % self.tiles[1];
        let z = index / (self.tiles[0] * self.tiles[1]);
        [0, 1, 2].map(|axis| {
            let offset = [x, y, z][axis] as u32 * TILE[axis];
            let start = self.ranges[axis].start + offset;
            start..(start + TILE[axis]).min(self.ranges[axis].end)
        })
    }

    pub fn add(
        &mut self,
        slot: usize,
        index: usize,
        a: &VoxelData,
        b: &VoxelData,
        mask: Option<&Mask>,
    ) {
        let tile = self.tile(index);
        let dims = [0, 1, 2].map(|axis| (tile[axis].end - tile[axis].start) as usize);
        let voxels = dims.iter().product::<usize>();
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "a colocalization tile carries both channels voxel for voxel"
        );
        let stride = self.stride as usize;
        let scales = self.scales;
        let grid = self.dims;
        let mut offsets = [0usize; 3];
        let mut starts = [0usize; 3];
        for axis in 0..3 {
            let relative = (tile[axis].start - self.ranges[axis].start) as usize;
            offsets[axis] = (stride - relative % stride) % stride;
            starts[axis] = (relative + offsets[axis]) / stride;
        }
        let frame = &mut self.frames[slot];
        let mut z = offsets[2];
        let mut compact_z = starts[2];
        while z < dims[2] {
            let mut y = offsets[1];
            let mut compact_y = starts[1];
            while y < dims[1] {
                let mut x = offsets[0];
                let mut compact_x = starts[0];
                while x < dims[0] {
                    let global = [
                        tile[0].start + x as u32,
                        tile[1].start + y as u32,
                        tile[2].start + z as u32,
                    ];
                    let voxel = x + dims[0] * (y + dims[1] * z);
                    let compact =
                        compact_x + grid[0] as usize * (compact_y + grid[1] as usize * compact_z);
                    frame.a[compact] = unit(scales[0], a.value(voxel));
                    frame.b[compact] = unit(scales[1], b.value(voxel));
                    if mask.is_none_or(|mask| mask.contains_cell(global)) {
                        frame.included.push(
                            u32::try_from(compact)
                                .expect("a colocalization sample grid fits the u32 index"),
                        );
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
    }

    pub fn sampled_voxels(&self) -> u64 {
        self.dims.iter().map(|dim| u64::from(*dim)).product()
    }

    pub fn sampled_frames(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn blocks(&self) -> u64 {
        self.blocks_per_frame() as u64 * self.frames.len() as u64
    }

    pub fn randomizations(&self) -> u32 {
        self.settings.permutations()
    }

    pub fn test(self) -> Option<ColocSignificance> {
        let randomizations = self.settings.permutations();
        let mut observed = Totals::default();
        for frame in &self.frames {
            for index in &frame.included {
                let index = *index as usize;
                observed.push(frame.a[index], frame.b[index]);
            }
        }
        let observed_pearson = observed.pearson()?;
        let dims = self.dims;
        let block = self.block;
        let blocks = self.blocks_per_frame();
        let mut rng = SplitMix64::new(seed(self.pair, self.level, self.stride));
        let mut order = (0..blocks as u32).collect::<Vec<_>>();
        let mut shuffled = vec![0.0f32; self.frames[0].a.len()];
        let mut count = 0u64;
        for _ in 0..randomizations {
            let mut totals = Totals::default();
            for frame in &self.frames {
                shuffle(&mut order, &mut rng);
                fill(&mut shuffled, &frame.a, dims, block, &order);
                for index in &frame.included {
                    let index = *index as usize;
                    totals.push(shuffled[index], frame.b[index]);
                }
            }
            if totals
                .pearson()
                .is_some_and(|pearson| pearson >= observed_pearson - EPSILON)
            {
                count += 1;
            }
        }
        let p = (1.0 + count as f64) / (1.0 + f64::from(randomizations));
        Some(ColocSignificance::new(p, randomizations, observed_pearson))
    }

    fn blocks_per_frame(&self) -> usize {
        [0, 1, 2]
            .map(|axis| self.dims[axis].div_ceil(self.block[axis]) as usize)
            .iter()
            .product()
    }
}

#[derive(Clone, Copy, Default)]
struct Totals {
    count: u64,
    a: f64,
    b: f64,
    aa: f64,
    bb: f64,
    ab: f64,
}

impl Totals {
    fn push(&mut self, a: f32, b: f32) {
        let a = f64::from(a);
        let b = f64::from(b);
        self.count += 1;
        self.a += a;
        self.b += b;
        self.aa += a * a;
        self.bb += b * b;
        self.ab += a * b;
    }

    fn pearson(self) -> Option<f64> {
        if self.count < MINIMUM {
            return None;
        }
        let count = self.count as f64;
        let left = count * self.aa - self.a * self.a;
        let right = count * self.bb - self.b * self.b;
        if left <= 0.0 || right <= 0.0 {
            return None;
        }
        Some(((count * self.ab - self.a * self.b) / (left * right).sqrt()).clamp(-1.0, 1.0))
    }
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn seed(pair: ColocPair, level: usize, stride: u32) -> u64 {
    SEED ^ ((pair.a() as u64) << 40)
        ^ ((pair.b() as u64) << 20)
        ^ ((level as u64) << 8)
        ^ u64::from(stride)
}

fn shuffle(order: &mut [u32], rng: &mut SplitMix64) {
    for index in (1..order.len()).rev() {
        let other = rng.below(index + 1);
        order.swap(index, other);
    }
}

fn fill(shuffled: &mut [f32], source: &[f32], dims: [u32; 3], block: [u32; 3], order: &[u32]) {
    let blocks = [
        dims[0].div_ceil(block[0]),
        dims[1].div_ceil(block[1]),
        dims[2].div_ceil(block[2]),
    ];
    #[cfg(debug_assertions)]
    let mut written = 0usize;
    for (destination, origin) in order.iter().enumerate() {
        let origin = decode(*origin, blocks);
        let target = decode(destination as u32, blocks);
        for local_z in 0..block[2] {
            let z = target[2] * block[2] + local_z;
            if z >= dims[2] {
                break;
            }
            let source_z = mirror(origin[2] * block[2] + local_z, dims[2]);
            for local_y in 0..block[1] {
                let y = target[1] * block[1] + local_y;
                if y >= dims[1] {
                    break;
                }
                let source_y = mirror(origin[1] * block[1] + local_y, dims[1]);
                for local_x in 0..block[0] {
                    let x = target[0] * block[0] + local_x;
                    if x >= dims[0] {
                        break;
                    }
                    let source_x = mirror(origin[0] * block[0] + local_x, dims[0]);
                    debug_assert!(
                        x < dims[0] && y < dims[1] && z < dims[2],
                        "a randomization writes target cells inside the sampled grid"
                    );
                    debug_assert!(
                        source_x < dims[0] && source_y < dims[1] && source_z < dims[2],
                        "a randomization reads mirrored source cells inside the sampled grid"
                    );
                    let target_index = (x + dims[0] * (y + dims[1] * z)) as usize;
                    let source_index =
                        (source_x + dims[0] * (source_y + dims[1] * source_z)) as usize;
                    shuffled[target_index] = source[source_index];
                    #[cfg(debug_assertions)]
                    {
                        written += 1;
                    }
                }
            }
        }
    }
    #[cfg(debug_assertions)]
    debug_assert_eq!(
        written,
        shuffled.len(),
        "a randomization fills every sampled voxel"
    );
}

fn mirror(index: u32, dim: u32) -> u32 {
    debug_assert!(dim > 0, "a sampled grid spans a positive extent");
    if dim == 1 {
        return 0;
    }
    let period = 2 * (dim - 1);
    let folded = index % period;
    if folded < dim {
        folded
    } else {
        period - folded
    }
}

fn decode(index: u32, blocks: [u32; 3]) -> [u32; 3] {
    [
        index % blocks[0],
        (index / blocks[0]) % blocks[1],
        index / (blocks[0] * blocks[1]),
    ]
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
