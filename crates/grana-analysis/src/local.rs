use grana_volume::Intensity;

pub(crate) const BLOCK: i64 = 8;
pub(crate) const WINDOW: i64 = 2;

const BACKGROUND_QUANTILE: f64 = 0.5;
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

pub(crate) struct LocalField(Grid);

impl LocalField {
    pub(crate) fn new(
        dims: [usize; 3],
        start: [i64; 3],
        anchor: [i64; 3],
        intensity: Intensity,
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
                    if bucket.is_empty() {
                        continue;
                    }
                    let total = bucket.len();
                    let background_rank =
                        ((total as f64 * BACKGROUND_QUANTILE) as usize).min(total - 1);
                    let background_level = *bucket
                        .select_nth_unstable_by(background_rank, f32::total_cmp)
                        .1;
                    let signal_rank = ((total as f64 * SIGNAL_QUANTILE) as usize).min(total - 1);
                    let signal_level =
                        *bucket.select_nth_unstable_by(signal_rank, f32::total_cmp).1;
                    let index = block_x + blocks[0] * (block_y + blocks[1] * block_z);
                    background[index] = background_level;
                    signal[index] = signal_level;
                }
            }
        }
        window_median(&mut background, blocks);
        window_max(&mut signal, blocks);
        fill_missing(&mut background, blocks, intensity.max());
        fill_missing(&mut signal, blocks, intensity.max());
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

    pub(crate) fn extrapolate(&mut self) {
        extrapolate(&mut self.0.background, self.0.blocks);
        extrapolate(&mut self.0.signal, self.0.blocks);
    }
}

fn window_median(values: &mut [f32], blocks: [usize; 3]) {
    let mut filtered = vec![0.0f32; values.len()];
    let mut window = Vec::with_capacity((2 * WINDOW as usize + 1).pow(3));
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                window.clear();
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let neighbour = [
                                (x + dx).clamp(0, blocks[0] as i64 - 1) as usize,
                                (y + dy).clamp(0, blocks[1] as i64 - 1) as usize,
                                (z + dz).clamp(0, blocks[2] as i64 - 1) as usize,
                            ];
                            let value = values[neighbour[0]
                                + blocks[0] * (neighbour[1] + blocks[1] * neighbour[2])];
                            if value.is_finite() {
                                window.push(value);
                            }
                        }
                    }
                }
                let slot = x as usize + blocks[0] * (y as usize + blocks[1] * z as usize);
                if window.is_empty() {
                    filtered[slot] = INVALID;
                } else {
                    window.sort_unstable_by(f32::total_cmp);
                    filtered[slot] = window[window.len() / 2];
                }
            }
        }
    }
    values.copy_from_slice(&filtered);
}

fn window_max(values: &mut [f32], blocks: [usize; 3]) {
    let mut filtered = vec![0.0f32; values.len()];
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                let mut best = INVALID;
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let neighbour = [
                                (x + dx).clamp(0, blocks[0] as i64 - 1) as usize,
                                (y + dy).clamp(0, blocks[1] as i64 - 1) as usize,
                                (z + dz).clamp(0, blocks[2] as i64 - 1) as usize,
                            ];
                            let value = values[neighbour[0]
                                + blocks[0] * (neighbour[1] + blocks[1] * neighbour[2])];
                            if value.is_finite() {
                                best = if best.is_finite() {
                                    best.max(value)
                                } else {
                                    value
                                };
                            }
                        }
                    }
                }
                let slot = x as usize + blocks[0] * (y as usize + blocks[1] * z as usize);
                filtered[slot] = best;
            }
        }
    }
    values.copy_from_slice(&filtered);
}

fn fill_missing(values: &mut [f32], blocks: [usize; 3], fallback: f32) {
    for _ in 0..3 {
        if values.iter().all(|value| value.is_finite()) {
            return;
        }
        let previous = values.to_vec();
        for z in 0..blocks[2] as i64 {
            for y in 0..blocks[1] as i64 {
                for x in 0..blocks[0] as i64 {
                    let slot = x as usize + blocks[0] * (y as usize + blocks[1] * z as usize);
                    if previous[slot].is_finite() {
                        continue;
                    }
                    let mut total = 0.0f32;
                    let mut count = 0u32;
                    for (dx, dy, dz) in [
                        (1, 0, 0),
                        (-1, 0, 0),
                        (0, 1, 0),
                        (0, -1, 0),
                        (0, 0, 1),
                        (0, 0, -1),
                    ] {
                        let neighbour = [x + dx, y + dy, z + dz];
                        if (0..3).any(|axis| {
                            neighbour[axis] < 0 || neighbour[axis] >= blocks[axis] as i64
                        }) {
                            continue;
                        }
                        let value = previous[neighbour[0] as usize
                            + blocks[0]
                                * (neighbour[1] as usize + blocks[1] * neighbour[2] as usize)];
                        if value.is_finite() {
                            total += value;
                            count += 1;
                        }
                    }
                    if count > 0 {
                        values[slot] = total / count as f32;
                    }
                }
            }
        }
    }
    for value in values {
        if !value.is_finite() {
            *value = fallback;
        }
    }
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
