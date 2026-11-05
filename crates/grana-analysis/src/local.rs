use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use grana_volume::Intensity;

pub(crate) const BLOCK: i64 = 8;
pub(crate) const WINDOW: i64 = 2;

const SIGNAL_QUANTILE: f64 = 0.995;
const MINIMUM_RANGE: f32 = 0.02;
const INVALID: f32 = f32::NAN;
const MARGIN: i64 = 1;

pub(crate) fn floor(intensity: Intensity) -> f32 {
    intensity.span() * MINIMUM_RANGE
}

pub(crate) struct Grid {
    anchor: [i64; 3],
    origin: [i64; 3],
    blocks: [usize; 3],
    background: Vec<f32>,
    signal: Vec<f32>,
    floor: f32,
}

impl Grid {
    pub(crate) fn contrast(&self, voxel: [i64; 3], value: f32) -> f32 {
        let mut index = [[0usize; 2]; 3];
        let mut weight = [[0.0f32; 2]; 3];
        for axis in 0..3 {
            let position = 2 * (voxel[axis] - self.anchor[axis]) + 1;
            let first = (position - BLOCK).div_euclid(2 * BLOCK);
            let fraction = (position - (first * 2 * BLOCK + BLOCK)) as f32 / (2 * BLOCK) as f32;
            for slot in 0..2 {
                let local = (first + slot as i64 - self.origin[axis])
                    .clamp(0, self.blocks[axis] as i64 - 1);
                index[axis][slot] = local as usize;
                weight[axis][slot] = if slot == 0 { 1.0 - fraction } else { fraction };
            }
        }
        let mut background = 0.0f32;
        let mut signal = 0.0f32;
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let share = weight[0][x] * weight[1][y] * weight[2][z];
                    let slot =
                        index[0][x] + self.blocks[0] * (index[1][y] + self.blocks[1] * index[2][z]);
                    background += self.background[slot] * share;
                    signal += self.signal[slot] * share;
                }
            }
        }
        (value - background) / (signal - background).max(self.floor)
    }
}

pub(crate) struct Stats {
    origin: [i64; 3],
    blocks: [usize; 3],
    values: Vec<AtomicU64>,
    ready: Vec<AtomicBool>,
}

impl Stats {
    pub(crate) fn new(low: [i64; 3], high: [i64; 3]) -> Self {
        let origin = low;
        let blocks = [0, 1, 2].map(|axis| (high[axis] - low[axis] + 1).max(1) as usize);
        let count = blocks[0] * blocks[1] * blocks[2];
        Self {
            origin,
            blocks,
            values: (0..count).map(|_| AtomicU64::new(0)).collect(),
            ready: (0..count).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    fn slot(&self, block: [i64; 3]) -> Option<usize> {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let offset = block[axis] - self.origin[axis];
            if offset < 0 || offset >= self.blocks[axis] as i64 {
                return None;
            }
            local[axis] = offset as usize;
        }
        Some(local[0] + self.blocks[0] * (local[1] + self.blocks[1] * local[2]))
    }

    fn fetch(&self, block: [i64; 3]) -> Option<(f32, f32)> {
        let slot = self.slot(block)?;
        if !self.ready[slot].load(Ordering::Acquire) {
            return None;
        }
        let packed = self.values[slot].load(Ordering::Relaxed);
        Some((
            f32::from_bits((packed >> 32) as u32),
            f32::from_bits(packed as u32),
        ))
    }

    fn store(&self, block: [i64; 3], level: (f32, f32)) {
        let Some(slot) = self.slot(block) else {
            return;
        };
        let packed = (u64::from(level.0.to_bits()) << 32) | u64::from(level.1.to_bits());
        self.values[slot].store(packed, Ordering::Relaxed);
        self.ready[slot].store(true, Ordering::Release);
    }
}

pub(crate) struct LocalField(Grid);

impl LocalField {
    pub(crate) fn new(
        dims: [usize; 3],
        start: [i64; 3],
        anchor: [i64; 3],
        intensity: Intensity,
        value: impl Fn(usize) -> f32,
        admits: impl Fn([i64; 3]) -> bool,
        stats: Option<&Stats>,
    ) -> Self {
        assert!(
            dims.iter().all(|axis| *axis > 0),
            "a local field covers a non empty array"
        );
        let mut origin = [0i64; 3];
        let mut blocks = [0usize; 3];
        for axis in 0..3 {
            let first = (start[axis] - anchor[axis]).div_euclid(BLOCK);
            let last = (start[axis] + dims[axis] as i64 - 1 - anchor[axis]).div_euclid(BLOCK);
            origin[axis] = first;
            blocks[axis] = (last - first + 1) as usize;
        }
        let count = blocks[0] * blocks[1] * blocks[2];
        let mut background = vec![INVALID; count];
        let mut signal = vec![INVALID; count];
        let mut bucket = Vec::with_capacity((BLOCK * BLOCK * BLOCK) as usize);
        for block_z in 0..blocks[2] {
            for block_y in 0..blocks[1] {
                for block_x in 0..blocks[0] {
                    let mut low = [0usize; 3];
                    let mut high = [0usize; 3];
                    let mut whole = true;
                    for axis in 0..3 {
                        let index = [block_x, block_y, block_z][axis] as i64 + origin[axis];
                        let first = index * BLOCK + anchor[axis] - start[axis];
                        whole &= first >= 0 && first + BLOCK <= dims[axis] as i64;
                        low[axis] = first.max(0) as usize;
                        high[axis] = (first + BLOCK).min(dims[axis] as i64) as usize;
                    }
                    let block = [
                        origin[0] + block_x as i64,
                        origin[1] + block_y as i64,
                        origin[2] + block_z as i64,
                    ];
                    let index = block_x + blocks[0] * (block_y + blocks[1] * block_z);
                    if let Some(stats) = stats
                        && whole
                        && let Some(level) = stats.fetch(block)
                    {
                        background[index] = level.0;
                        signal[index] = level.1;
                        continue;
                    }
                    bucket.clear();
                    for z in low[2]..high[2] {
                        for y in low[1]..high[1] {
                            let base = dims[0] * (y + dims[1] * z);
                            for x in low[0]..high[0] {
                                let voxel = [
                                    start[0] + x as i64,
                                    start[1] + y as i64,
                                    start[2] + z as i64,
                                ];
                                if admits(voxel) {
                                    bucket.push(value(base + x));
                                }
                            }
                        }
                    }
                    if !bucket.is_empty() {
                        let total = bucket.len();
                        let background_rank = total / 2;
                        let background_level = *bucket
                            .select_nth_unstable_by(background_rank, f32::total_cmp)
                            .1;
                        let signal_rank =
                            ((total as f64 * SIGNAL_QUANTILE) as usize).min(total - 1);
                        let signal_level =
                            *bucket.select_nth_unstable_by(signal_rank, f32::total_cmp).1;
                        background[index] = background_level;
                        signal[index] = signal_level;
                        if let Some(stats) = stats
                            && whole
                        {
                            stats.store(block, (background_level, signal_level));
                        }
                    }
                }
            }
        }
        let windowed = window_median(&background, blocks);
        for (level, window) in background.iter_mut().zip(&windowed) {
            *level = level.min(*window);
        }
        window_max(&mut signal, blocks);
        fill(&mut background, blocks, intensity.max());
        fill(&mut signal, blocks, intensity.max());
        let padded = blocks.map(|value| value + 2);
        let mut background = pad(&background, padded);
        let mut signal = pad(&signal, padded);
        extrapolate(&mut background, padded);
        extrapolate(&mut signal, padded);
        Self(Grid {
            anchor,
            origin: origin.map(|value| value - MARGIN),
            blocks: padded,
            background,
            signal,
            floor: floor(intensity),
        })
    }

    pub(crate) fn blocks(&self) -> ([i64; 3], [usize; 3]) {
        (self.0.origin, self.0.blocks)
    }

    pub(crate) fn block(&self, local: usize) -> (f32, f32) {
        (self.0.background[local], self.0.signal[local])
    }

    pub(crate) fn contrast(&self, voxel: [i64; 3], value: f32) -> f32 {
        self.0.contrast(voxel, value)
    }
}

pub(crate) struct BlockField(Grid);

impl BlockField {
    pub(crate) fn new(anchor: [i64; 3], blocks: [usize; 3], floor: f32) -> Self {
        let blocks = blocks.map(|value| value + 2);
        let count = blocks[0] * blocks[1] * blocks[2];
        Self(Grid {
            anchor,
            origin: [-MARGIN; 3],
            blocks,
            background: vec![0.0; count],
            signal: vec![0.0; count],
            floor,
        })
    }

    pub(crate) fn set(&mut self, block: [i64; 3], value: (f32, f32)) {
        let index = self.index(block);
        self.0.background[index] = value.0;
        self.0.signal[index] = value.1;
    }

    fn index(&self, block: [i64; 3]) -> usize {
        let local = [0, 1, 2].map(|axis| {
            (block[axis] - self.0.origin[axis]).clamp(0, self.0.blocks[axis] as i64 - 1) as usize
        });
        local[0] + self.0.blocks[0] * (local[1] + self.0.blocks[1] * local[2])
    }

    pub(crate) fn contrast(&self, voxel: [i64; 3], value: f32) -> f32 {
        self.0.contrast(voxel, value)
    }

    pub(crate) fn at(&self, block: [i64; 3]) -> (f32, f32) {
        let index = self.index(block);
        (self.0.background[index], self.0.signal[index])
    }

    pub(crate) fn floor(&self) -> f32 {
        self.0.floor
    }

    pub(crate) fn extrapolate(&mut self) {
        extrapolate(&mut self.0.background, self.0.blocks);
        extrapolate(&mut self.0.signal, self.0.blocks);
    }
}

fn window_median(values: &[f32], blocks: [usize; 3]) -> Vec<f32> {
    let mut filtered = vec![INVALID; values.len()];
    let mut window = Vec::with_capacity((2 * WINDOW as usize + 1).pow(3));
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                window.clear();
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let value = values[neighbour_slot([x + dx, y + dy, z + dz], blocks)];
                            if value.is_finite() {
                                window.push(value);
                            }
                        }
                    }
                }
                if window.is_empty() {
                    continue;
                }
                let rank = window.len() / 2;
                window.select_nth_unstable_by(rank, f32::total_cmp);
                let slot = block_slot([x as usize, y as usize, z as usize], blocks);
                filtered[slot] = window[rank];
            }
        }
    }
    filtered
}

fn window_max(values: &mut [f32], blocks: [usize; 3]) {
    let mut filtered = vec![INVALID; values.len()];
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                let mut best = INVALID;
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let value = values[neighbour_slot([x + dx, y + dy, z + dz], blocks)];
                            if !value.is_finite() {
                                continue;
                            }
                            best = if best.is_finite() {
                                best.max(value)
                            } else {
                                value
                            };
                        }
                    }
                }
                let slot = block_slot([x as usize, y as usize, z as usize], blocks);
                filtered[slot] = best;
            }
        }
    }
    values.copy_from_slice(&filtered);
}

fn fill(values: &mut [f32], blocks: [usize; 3], fallback: f32) {
    let mut queue = VecDeque::new();
    let mut visited = vec![false; values.len()];
    for (slot, value) in values.iter().enumerate() {
        if value.is_finite() {
            visited[slot] = true;
            queue.push_back(slot);
        }
    }
    if queue.is_empty() {
        values.fill(fallback);
        return;
    }
    while let Some(slot) = queue.pop_front() {
        let block = [
            slot % blocks[0],
            (slot / blocks[0]) % blocks[1],
            slot / (blocks[0] * blocks[1]),
        ];
        let value = values[slot];
        for axis in 0..3 {
            for step in [-1i64, 1] {
                let reached = block[axis] as i64 + step;
                if reached < 0 || reached >= blocks[axis] as i64 {
                    continue;
                }
                let mut neighbour = block;
                neighbour[axis] = reached as usize;
                let target = block_slot(neighbour, blocks);
                if visited[target] {
                    continue;
                }
                visited[target] = true;
                values[target] = value;
                queue.push_back(target);
            }
        }
    }
}

fn block_slot(block: [usize; 3], blocks: [usize; 3]) -> usize {
    block[0] + blocks[0] * (block[1] + blocks[1] * block[2])
}

fn neighbour_slot(block: [i64; 3], blocks: [usize; 3]) -> usize {
    block_slot(
        [
            block[0].clamp(0, blocks[0] as i64 - 1) as usize,
            block[1].clamp(0, blocks[1] as i64 - 1) as usize,
            block[2].clamp(0, blocks[2] as i64 - 1) as usize,
        ],
        blocks,
    )
}

fn pad(values: &[f32], padded: [usize; 3]) -> Vec<f32> {
    let inner = [padded[0] - 2, padded[1] - 2, padded[2] - 2];
    let mut out = vec![0.0f32; padded[0] * padded[1] * padded[2]];
    for z in 0..inner[2] {
        for y in 0..inner[1] {
            let source = inner[0] * (y + inner[1] * z);
            let target = padded[0] * (y + 1 + padded[1] * (z + 1)) + 1;
            out[target..target + inner[0]].copy_from_slice(&values[source..source + inner[0]]);
        }
    }
    out
}

fn extrapolate(values: &mut [f32], blocks: [usize; 3]) {
    let strides = [1, blocks[0], blocks[0] * blocks[1]];
    for axis in 0..3 {
        let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
        let length = blocks[axis];
        let last = length - 1;
        let mut line = vec![0.0f32; length];
        for position in 0..blocks[second] {
            for row in 0..blocks[first] {
                let base = strides[first] * row + strides[second] * position;
                for (step, slot) in line.iter_mut().enumerate() {
                    *slot = values[base + strides[axis] * step];
                }
                if last < 4 {
                    values[base] = line[1];
                    values[base + strides[axis] * last] = line[last - 1];
                    continue;
                }
                let forward = line[1] - line[2];
                let backward = line[last - 1] - line[last - 2];
                values[base] = line[1] - forward;
                values[base + strides[axis] * last] = line[last - 1] + backward;
            }
        }
    }
}
