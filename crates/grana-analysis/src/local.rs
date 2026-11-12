use std::collections::VecDeque;
use std::ops::Range;

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
    fn axis(&self, coordinate: i64, axis: usize) -> (i64, f32) {
        let position = 2 * (coordinate - self.anchor[axis]) + 1;
        let first = (position - BLOCK).div_euclid(2 * BLOCK);
        let fraction = (position - (first * 2 * BLOCK + BLOCK)) as f32 / (2 * BLOCK) as f32;
        (first, fraction)
    }

    fn local(&self, first: i64, axis: usize) -> [usize; 2] {
        [0, 1].map(|slot| {
            (first + slot as i64 - self.origin[axis]).clamp(0, self.blocks[axis] as i64 - 1)
                as usize
        })
    }

    pub(crate) fn row(&self, y: i64, z: i64) -> Row<'_> {
        let (first_y, weight_y) = self.axis(y, 1);
        let (first_z, weight_z) = self.axis(z, 2);
        Row {
            grid: self,
            y: self.local(first_y, 1),
            z: self.local(first_z, 2),
            y_weight: [1.0 - weight_y, weight_y],
            z_weight: [1.0 - weight_z, weight_z],
            span: None,
            background: [0.0; 2],
            signal: [0.0; 2],
        }
    }
}

pub struct Row<'a> {
    grid: &'a Grid,
    y: [usize; 2],
    z: [usize; 2],
    y_weight: [f32; 2],
    z_weight: [f32; 2],
    span: Option<i64>,
    background: [f32; 2],
    signal: [f32; 2],
}

impl Row<'_> {
    fn fill(&mut self, first: i64) {
        let grid: &Grid = self.grid;
        let spans = grid.local(first, 0);
        let blocks = grid.blocks;
        let [y0, y1] = self.y;
        let [z0, z1] = self.z;
        let [wy0, wy1] = self.y_weight;
        let [wz0, wz1] = self.z_weight;
        let slot = |x: usize, y: usize, z: usize| x + blocks[0] * (y + blocks[1] * z);
        for (index, x) in spans.into_iter().enumerate() {
            let background = |y: usize, z: usize| grid.background[slot(x, y, z)];
            let signal = |y: usize, z: usize| grid.signal[slot(x, y, z)];
            self.background[index] = (background(y0, z0) * wy0 + background(y1, z0) * wy1) * wz0
                + (background(y0, z1) * wy0 + background(y1, z1) * wy1) * wz1;
            self.signal[index] = (signal(y0, z0) * wy0 + signal(y1, z0) * wy1) * wz0
                + (signal(y0, z1) * wy0 + signal(y1, z1) * wy1) * wz1;
        }
        self.span = Some(first);
    }

    pub fn contrast(&mut self, x: i64, value: f32) -> f32 {
        let grid: &Grid = self.grid;
        let (first, fraction) = grid.axis(x, 0);
        if self.span != Some(first) {
            self.fill(first);
        }
        let first_weight = 1.0 - fraction;
        let other_weight = fraction;
        let background = self.background[0] * first_weight + self.background[1] * other_weight;
        let signal = self.signal[0] * first_weight + self.signal[1] * other_weight;
        (value - background) / (signal - background).max(grid.floor)
    }
}

pub(crate) fn block_levels(bucket: &mut [f32]) -> Option<(f32, f32)> {
    if bucket.is_empty() {
        return None;
    }
    let total = bucket.len();
    let background = *bucket.select_nth_unstable_by(total / 2, f32::total_cmp).1;
    let rank = ((total as f64 * SIGNAL_QUANTILE) as usize).min(total - 1);
    let signal = *bucket.select_nth_unstable_by(rank, f32::total_cmp).1;
    Some((background, signal))
}

pub(crate) struct LocalField(Grid);

const BUCKET: usize = (BLOCK * BLOCK * BLOCK) as usize;

pub struct BlockLevels {
    anchor: [i64; 3],
    region: [Range<u32>; 3],
    cover: [Range<u32>; 3],
    width: usize,
    height: usize,
    blocks: [usize; 3],
    intensity: Intensity,
    workers: usize,
    background: Vec<f32>,
    signal: Vec<f32>,
    buckets: Vec<f32>,
    counts: Vec<u32>,
    levels: Vec<Option<(f32, f32)>>,
    row: i64,
}

impl BlockLevels {
    pub fn new(
        anchor: [i64; 3],
        region: [Range<u32>; 3],
        cover: [Range<u32>; 3],
        intensity: Intensity,
        workers: usize,
    ) -> Self {
        assert!(
            region.iter().all(|range| !range.is_empty()),
            "a block field covers a non empty region"
        );
        let blocks = [0, 1, 2]
            .map(|axis| (region[axis].end - region[axis].start).div_ceil(BLOCK as u32) as usize);
        let columns = blocks[0] * blocks[1];
        let width = (cover[0].end - cover[0].start) as usize;
        let height = (cover[1].end - cover[1].start) as usize;
        Self {
            anchor,
            region,
            cover,
            width,
            height,
            blocks,
            intensity,
            workers: workers.max(1),
            background: vec![f32::NAN; columns * blocks[2]],
            signal: vec![f32::NAN; columns * blocks[2]],
            buckets: vec![0.0; columns * BUCKET],
            counts: vec![0; columns],
            levels: vec![None; columns],
            row: i64::MIN,
        }
    }

    pub fn push(
        &mut self,
        admits: &(dyn Fn([i64; 3]) -> bool + Sync),
        start: u32,
        values: &[f32],
        planes: usize,
    ) {
        let plane = self.width * self.height;
        assert!(
            !values.is_empty() && values.len() == plane * planes,
            "a detection band holds whole planes"
        );
        let end = start.saturating_add(planes as u32);
        let low = start.max(self.region[2].start);
        let high = end.min(self.region[2].end);
        let mut z = low;
        while z < high {
            let offset = i64::from(z) - self.anchor[2];
            let row = offset.div_euclid(BLOCK);
            let row_end = ((i64::from(z) + BLOCK - offset.rem_euclid(BLOCK)) as u32).min(high);
            if row != self.row {
                self.flush();
                self.row = row;
            }
            self.push_rows(admits, start, z, row_end, values);
            z = row_end;
        }
    }

    fn push_rows(
        &mut self,
        admits: &(dyn Fn([i64; 3]) -> bool + Sync),
        start: u32,
        low: u32,
        high: u32,
        values: &[f32],
    ) {
        let blocks = self.blocks;
        let region = &self.region;
        let cover = &self.cover;
        let width = self.width;
        let plane = self.width * self.height;
        let anchor = self.anchor;
        let chunk = blocks[1].div_ceil(self.workers).max(1);
        let counts = &mut self.counts[..];
        let buckets = &mut self.buckets[..];
        crate::schedule::parallel(|scope| {
            for (group, (counts, buckets)) in counts
                .chunks_mut(blocks[0] * chunk)
                .zip(buckets.chunks_mut(blocks[0] * chunk * BUCKET))
                .enumerate()
            {
                let first = group * chunk;
                scope.spawn(move |_| {
                    for (offset, (counts, buckets)) in counts
                        .chunks_mut(blocks[0])
                        .zip(buckets.chunks_mut(blocks[0] * BUCKET))
                        .enumerate()
                    {
                        let by = first + offset;
                        if by >= blocks[1] {
                            break;
                        }
                        let first_y = anchor[1] + (by * BLOCK as usize) as i64;
                        let y0 = first_y
                            .max(i64::from(region[1].start))
                            .max(i64::from(cover[1].start));
                        let y1 = (first_y + BLOCK)
                            .min(i64::from(region[1].end))
                            .min(i64::from(cover[1].end));
                        if y1 <= y0 {
                            continue;
                        }
                        let mut spans = Vec::with_capacity(blocks[0]);
                        for bx in 0..blocks[0] {
                            let first_x = anchor[0] + (bx * BLOCK as usize) as i64;
                            let x0 = first_x
                                .max(i64::from(region[0].start))
                                .max(i64::from(cover[0].start));
                            let x1 = (first_x + BLOCK)
                                .min(i64::from(region[0].end))
                                .min(i64::from(cover[0].end));
                            spans.push((x0, x1));
                        }
                        for z in low..high {
                            let base = (z - start) as usize * plane;
                            for (bx, (count, bucket)) in counts
                                .iter_mut()
                                .zip(buckets.chunks_mut(BUCKET))
                                .enumerate()
                            {
                                let (x0, x1) = spans[bx];
                                if x1 <= x0 {
                                    continue;
                                }
                                let mut stored = *count as usize;
                                for y in y0..y1 {
                                    let origin = (y - i64::from(cover[1].start)) as usize * width;
                                    for x in x0..x1 {
                                        if !admits([x, y, i64::from(z)]) {
                                            continue;
                                        }
                                        let slot =
                                            origin + (x - i64::from(cover[0].start)) as usize;
                                        bucket[stored] = values[base + slot];
                                        stored += 1;
                                    }
                                }
                                assert!(
                                    stored <= BUCKET,
                                    "an eight voxel block fills its bucket once"
                                );
                                *count = stored as u32;
                            }
                        }
                    }
                });
            }
        });
    }

    pub fn finish(mut self) -> BlockField {
        self.flush();
        BlockField::from_blocks(
            self.anchor,
            self.blocks,
            self.intensity,
            self.background,
            self.signal,
            self.workers,
        )
    }

    fn flush(&mut self) {
        if self.row == i64::MIN {
            return;
        }
        let blocks = self.blocks;
        let columns = blocks[0] * blocks[1];
        let chunk = columns.div_ceil(self.workers).max(1);
        {
            let buckets = &mut self.buckets[..];
            let counts = &self.counts[..];
            let levels = &mut self.levels[..];
            crate::schedule::parallel(|scope| {
                for ((bucket, counts), levels) in buckets
                    .chunks_mut(chunk * BUCKET)
                    .zip(counts.chunks(chunk))
                    .zip(levels.chunks_mut(chunk))
                {
                    scope.spawn(move |_| {
                        for (bucket, (count, level)) in bucket
                            .chunks_mut(BUCKET)
                            .zip(counts.iter().zip(levels.iter_mut()))
                        {
                            *level = block_levels(&mut bucket[..*count as usize]);
                        }
                    });
                }
            });
        }
        let z = self.row as usize;
        for (cell, level) in self.levels.iter().enumerate() {
            let Some((background, signal)) = level else {
                continue;
            };
            let bx = cell % blocks[0];
            let by = cell / blocks[0];
            let slot = bx + blocks[0] * (by + blocks[1] * z);
            self.background[slot] = *background;
            self.signal[slot] = *signal;
        }
        self.levels.fill(None);
        self.counts.fill(0);
    }
}

impl LocalField {
    pub(crate) fn new(
        dims: [usize; 3],
        start: [i64; 3],
        anchor: [i64; 3],
        intensity: Intensity,
        workers: usize,
        value: impl Fn(usize) -> f32,
        admits: impl Fn([i64; 3]) -> bool,
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
                    for axis in 0..3 {
                        let index = [block_x, block_y, block_z][axis] as i64 + origin[axis];
                        let first = index * BLOCK + anchor[axis] - start[axis];
                        low[axis] = first.max(0) as usize;
                        high[axis] = (first + BLOCK).min(dims[axis] as i64) as usize;
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
                    let index = block_x + blocks[0] * (block_y + blocks[1] * block_z);
                    if let Some((background_level, signal_level)) = block_levels(&mut bucket) {
                        background[index] = background_level;
                        signal[index] = signal_level;
                    }
                }
            }
        }
        Self(finish(
            anchor, origin, blocks, intensity, background, signal, workers,
        ))
    }

    pub fn row(&self, y: i64, z: i64) -> Row<'_> {
        self.0.row(y, z)
    }
}

fn finish(
    anchor: [i64; 3],
    origin: [i64; 3],
    blocks: [usize; 3],
    intensity: Intensity,
    mut background: Vec<f32>,
    mut signal: Vec<f32>,
    workers: usize,
) -> Grid {
    let windowed = window_median(&background, blocks, workers);
    for (level, window) in background.iter_mut().zip(&windowed) {
        *level = level.min(*window);
    }
    let superseded = window_max(&signal, blocks, workers);
    signal.copy_from_slice(&superseded);
    fill(&mut background, blocks, intensity.max());
    fill(&mut signal, blocks, intensity.max());
    let padded = blocks.map(|value| value + 2);
    let mut background = pad(&background, padded);
    let mut signal = pad(&signal, padded);
    extrapolate(&mut background, padded);
    extrapolate(&mut signal, padded);
    Grid {
        anchor,
        origin: origin.map(|value| value - MARGIN),
        blocks: padded,
        background,
        signal,
        floor: floor(intensity),
    }
}

pub struct BlockField(Grid);

impl BlockField {
    pub fn from_blocks(
        anchor: [i64; 3],
        blocks: [usize; 3],
        intensity: Intensity,
        background: Vec<f32>,
        signal: Vec<f32>,
        workers: usize,
    ) -> Self {
        assert_eq!(
            background.len(),
            blocks[0] * blocks[1] * blocks[2],
            "a block field holds one level per block"
        );
        assert_eq!(
            signal.len(),
            background.len(),
            "a block field holds a signal level per background level"
        );
        Self(finish(
            anchor, [0; 3], blocks, intensity, background, signal, workers,
        ))
    }

    fn index(&self, block: [i64; 3]) -> usize {
        let local = [0, 1, 2].map(|axis| {
            (block[axis] - self.0.origin[axis]).clamp(0, self.0.blocks[axis] as i64 - 1) as usize
        });
        local[0] + self.0.blocks[0] * (local[1] + self.0.blocks[1] * local[2])
    }

    pub fn row(&self, y: i64, z: i64) -> Row<'_> {
        self.0.row(y, z)
    }

    pub fn at(&self, block: [i64; 3]) -> (f32, f32) {
        let index = self.index(block);
        (self.0.background[index], self.0.signal[index])
    }

    pub fn floor(&self) -> f32 {
        self.0.floor
    }
}

fn window_median(values: &[f32], blocks: [usize; 3], workers: usize) -> Vec<f32> {
    let plane = blocks[0] * blocks[1];
    let slices = (0..blocks[2]).collect::<Vec<_>>();
    let lines = crate::schedule::map(&slices, workers, |z| {
        let z = *z as i64;
        let mut window = Vec::with_capacity((2 * WINDOW as usize + 1).pow(3));
        let mut line = vec![INVALID; plane];
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
                line[(x + blocks[0] as i64 * y) as usize] = window[rank];
            }
        }
        line
    });
    let mut filtered = vec![INVALID; values.len()];
    for (slot, line) in filtered.chunks_mut(plane).zip(&lines) {
        slot.copy_from_slice(line);
    }
    filtered
}

fn window_max(values: &[f32], blocks: [usize; 3], workers: usize) -> Vec<f32> {
    let plane = blocks[0] * blocks[1];
    let slices = (0..blocks[2]).collect::<Vec<_>>();
    let lines = crate::schedule::map(&slices, workers, |z| {
        let z = *z as i64;
        let mut line = vec![INVALID; plane];
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
                line[(x + blocks[0] as i64 * y) as usize] = best;
            }
        }
        line
    });
    let mut filtered = vec![INVALID; values.len()];
    for (slot, line) in filtered.chunks_mut(plane).zip(&lines) {
        slot.copy_from_slice(line);
    }
    filtered
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
