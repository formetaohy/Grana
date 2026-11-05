use std::ops::Range;
use std::sync::atomic::AtomicBool;

use grana_measure::{ColocPair, ColocSettings, ColocSignificance};
use grana_volume::{Intensity, Level, Pyramid, Region};

use crate::schedule;
use grana_object::Mask;

use super::grid::Grid;
use super::{ColocLevels, ColocRead, scale, unit};

const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const EPSILON: f64 = 1.0e-12;
const MINIMUM: u64 = 3;

struct Frame {
    a: Vec<f32>,
    b: Vec<f32>,
    included: Vec<u32>,
}

#[derive(Clone)]
struct Sampling {
    grid: Grid,
    dims: [u32; 3],
    block: [u32; 3],
    stride: u32,
}

impl Sampling {
    fn voxels(&self) -> usize {
        self.dims.iter().map(|dim| *dim as usize).product()
    }

    fn blocks(&self) -> usize {
        [0, 1, 2]
            .map(|axis| self.dims[axis].div_ceil(self.block[axis]) as usize)
            .iter()
            .product()
    }
}

pub struct ColocPermutation {
    pair: ColocPair,
    level: usize,
    stride: u32,
    settings: ColocSettings,
    sampling: Sampling,
    scales: [[f32; 2]; 2],
    frames: Vec<Frame>,
    loaded: Vec<bool>,
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
        let block = block_size(settings.block(), info, pyramid.level(0), stride, dims);
        let slots = levels.frames().len();
        Self {
            pair,
            level,
            stride,
            settings,
            sampling: Sampling {
                grid: Grid::new(ranges),
                dims,
                block,
                stride,
            },
            scales: intensities.map(scale),
            frames: (0..slots)
                .map(|_| Frame {
                    a: Vec::new(),
                    b: Vec::new(),
                    included: Vec::new(),
                })
                .collect(),
            loaded: vec![false; slots],
        }
    }

    pub fn tile_count(&self) -> usize {
        self.sampling.grid.tile_count()
    }

    pub fn tile(&self, index: usize) -> [Range<u32>; 3] {
        self.sampling.grid.tile(index)
    }

    pub fn load(
        &mut self,
        times: &[u32],
        workers: usize,
        cancel: &AtomicBool,
        read: &ColocRead<'_>,
        mask: &(dyn Fn(u32) -> Result<Option<Mask>, String> + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<bool, String> {
        assert_eq!(
            times.len(),
            self.frames.len(),
            "the permutation samples one frame per slot"
        );
        let sampling = &self.sampling;
        let pair = self.pair;
        let scales = self.scales;
        let (frames, loaded) = (&mut self.frames, &mut self.loaded);
        let completed = schedule::ordered(
            schedule::Run {
                total: times.len(),
                workers: schedule::workers(workers),
                cancel,
                progress,
            },
            || (),
            |(), slot| {
                let mask = mask(times[slot])?;
                let mut frame = Frame {
                    a: vec![0.0; sampling.voxels()],
                    b: vec![0.0; sampling.voxels()],
                    included: Vec::with_capacity(sampling.voxels()),
                };
                sample(
                    sampling,
                    pair,
                    scales,
                    mask.as_ref(),
                    read,
                    times[slot],
                    &mut frame,
                )?;
                Ok(frame)
            },
            |slot, mut frame| {
                std::mem::swap(&mut frames[slot].a, &mut frame.a);
                std::mem::swap(&mut frames[slot].b, &mut frame.b);
                std::mem::swap(&mut frames[slot].included, &mut frame.included);
                loaded[slot] = true;
                Ok(())
            },
            |()| {},
        )?;
        Ok(completed)
    }

    pub fn sampled_voxels(&self) -> u64 {
        self.sampling.voxels() as u64
    }

    pub fn sampled_frames(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn blocks(&self) -> u64 {
        self.sampling.blocks() as u64 * self.frames.len() as u64
    }

    pub fn randomizations(&self) -> u32 {
        self.settings.permutations()
    }

    pub fn test(
        self,
        workers: usize,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Option<ColocSignificance> {
        assert!(
            self.loaded.iter().all(|loaded| *loaded),
            "the significance test runs on a loaded permutation grid"
        );
        let observed = observed_pearson(&self.frames)?;
        let randomizations = self.randomizations();
        let frames = &self.frames;
        let dims = self.sampling.dims;
        let block = self.sampling.block;
        let base = seed(self.pair, self.level, self.stride);
        let mut count = 0u64;
        let completed = schedule::ordered(
            schedule::Run {
                total: randomizations as usize,
                workers: schedule::workers(workers),
                cancel,
                progress,
            },
            || TestWorker::new(self.sampling.blocks(), self.sampling.voxels()),
            |worker, index| {
                let randomization = u32::try_from(index)
                    .expect("a colocalization randomization fits the u32 range");
                Ok(worker.randomize(
                    frames,
                    dims,
                    block,
                    observed,
                    randomization_seed(base, randomization),
                ))
            },
            |_, significant| {
                if significant {
                    count += 1;
                }
                Ok(())
            },
            |_| {},
        )
        .ok()?;
        if !completed {
            return None;
        }
        let p = (1.0 + count as f64) / (1.0 + f64::from(randomizations));
        Some(ColocSignificance::new(p, randomizations, observed))
    }
}

fn sample(
    sampling: &Sampling,
    pair: ColocPair,
    scales: [[f32; 2]; 2],
    mask: Option<&Mask>,
    read: &ColocRead<'_>,
    time: u32,
    frame: &mut Frame,
) -> Result<(), String> {
    let stride = sampling.stride as usize;
    for index in 0..sampling.grid.tile_count() {
        let tile = sampling.grid.tile(index);
        let a = read(time, pair.a(), &tile)?;
        let b = read(time, pair.b(), &tile)?;
        let tile_dims = [0, 1, 2].map(|axis| (tile[axis].end - tile[axis].start) as usize);
        let voxels = tile_dims.iter().product::<usize>();
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "a colocalization tile carries both channels voxel for voxel"
        );
        let mut offsets = [0usize; 3];
        let mut starts = [0usize; 3];
        for axis in 0..3 {
            let relative = (tile[axis].start - sampling.grid.ranges()[axis].start) as usize;
            offsets[axis] = (stride - relative % stride) % stride;
            starts[axis] = (relative + offsets[axis]) / stride;
        }
        let mut z = offsets[2];
        let mut compact_z = starts[2];
        while z < tile_dims[2] {
            let mut y = offsets[1];
            let mut compact_y = starts[1];
            while y < tile_dims[1] {
                let mut x = offsets[0];
                let mut compact_x = starts[0];
                while x < tile_dims[0] {
                    let global = [
                        tile[0].start + x as u32,
                        tile[1].start + y as u32,
                        tile[2].start + z as u32,
                    ];
                    let voxel = x + tile_dims[0] * (y + tile_dims[1] * z);
                    let compact = compact_x
                        + sampling.dims[0] as usize
                            * (compact_y + sampling.dims[1] as usize * compact_z);
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
    Ok(())
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

fn observed_pearson(frames: &[Frame]) -> Option<f64> {
    let mut observed = Totals::default();
    for frame in frames {
        for index in &frame.included {
            let index = *index as usize;
            observed.push(frame.a[index], frame.b[index]);
        }
    }
    observed.pearson()
}

struct TestWorker {
    order: Vec<u32>,
    shuffled: Vec<f32>,
}

impl TestWorker {
    fn new(blocks: usize, voxels: usize) -> Self {
        Self {
            order: (0..blocks as u32).collect(),
            shuffled: vec![0.0f32; voxels],
        }
    }

    fn randomize(
        &mut self,
        frames: &[Frame],
        dims: [u32; 3],
        block: [u32; 3],
        observed: f64,
        seed: u64,
    ) -> bool {
        let mut rng = SplitMix64::new(seed);
        for (slot, value) in self.order.iter_mut().enumerate() {
            *value = slot as u32;
        }
        shuffle(&mut self.order, &mut rng);
        let mut totals = Totals::default();
        for frame in frames {
            fill(&mut self.shuffled, &frame.a, dims, block, &self.order);
            for index in &frame.included {
                let index = *index as usize;
                totals.push(self.shuffled[index], frame.b[index]);
            }
        }
        totals
            .pearson()
            .is_some_and(|pearson| pearson >= observed - EPSILON)
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

fn randomization_seed(base: u64, randomization: u32) -> u64 {
    SplitMix64::new(base ^ (u64::from(randomization) << 1 | 1)).next()
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
                    let target_index = (x + dims[0] * (y + dims[1] * z)) as usize;
                    let source_index =
                        (source_x + dims[0] * (source_y + dims[1] * source_z)) as usize;
                    shuffled[target_index] = source[source_index];
                }
            }
        }
    }
}

fn mirror(index: u32, dim: u32) -> u32 {
    assert!(dim > 0, "a sampled grid spans a positive extent");
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
