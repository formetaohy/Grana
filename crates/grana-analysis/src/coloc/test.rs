use std::sync::atomic::AtomicBool;

use grana_measure::ColocSignificance;

use crate::schedule;

use super::scan::FrameSample;

pub(super) const SEED: u64 = 0x9e37_79b9_7f4a_7c15;

const EPSILON: f64 = 1.0e-12;
const MINIMUM: u64 = 3;

pub(super) struct Input<'a> {
    pub frames: &'a [FrameSample],
    pub dims: [u32; 3],
    pub block: [u32; 3],
    pub blocks: u64,
    pub randomizations: u32,
    pub seed: u64,
    pub workers: usize,
    pub cancel: &'a AtomicBool,
}

pub(super) fn permutation(
    input: Input<'_>,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<ColocSignificance> {
    let observed = observed_pearson(input.frames)?;
    let randomizations = input.randomizations;
    let frames = input.frames;
    let dims = input.dims;
    let block = input.block;
    let blocks = usize::try_from(input.blocks).expect("a colocalization block count fits usize");
    let base = input.seed;
    let mut count = 0u64;
    let completed = schedule::ordered(
        schedule::Run {
            total: randomizations as usize,
            workers: schedule::workers(input.workers),
            cancel: input.cancel,
            progress,
        },
        || TestWorker::new(blocks, frames.first().map_or(0, |frame| frame.a.len())),
        |worker, index| {
            let randomization =
                u32::try_from(index).expect("a colocalization randomization fits the u32 range");
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
    Some(ColocSignificance::new(p, randomizations))
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

fn observed_pearson(frames: &[FrameSample]) -> Option<f64> {
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
        frames: &[FrameSample],
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
