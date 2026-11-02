use grana_volume::Intensity;

pub(crate) const BLOCK: i64 = 8;

const BACKGROUND_QUANTILE: f64 = 0.5;
const SIGNAL_QUANTILE: f64 = 0.995;
const MINIMUM_RANGE: f32 = 0.02;
pub(crate) const WINDOW: i64 = 2;

pub(crate) const PAD_BLOCKS: i64 = WINDOW + 1;
const WINDOW_SIDE: i64 = 2 * WINDOW + 1;
const WINDOW_VOXELS: usize = (WINDOW_SIDE * WINDOW_SIDE * WINDOW_SIDE) as usize;

pub(crate) struct LocalField {
    origin: [i64; 3],
    blocks: [usize; 3],
    anchor: [i64; 3],
    background: Vec<f32>,
    signal: Vec<f32>,
    floor: f32,
}

impl LocalField {
    pub(crate) fn new(
        dims: [usize; 3],
        start: [i64; 3],
        anchor: [i64; 3],
        intensity: Intensity,
        value: impl Fn(usize) -> f32,
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
        let mut background = vec![0.0f32; count];
        let mut signal = vec![0.0f32; count];
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
                                bucket.push(value(base + x));
                            }
                        }
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
        let mut background = window_median(&background, blocks);
        let mut signal = window_max(&signal, blocks);
        for axis in 0..3 {
            background = extrapolate(background, blocks, axis);
            signal = extrapolate(signal, blocks, axis);
            blocks[axis] += 2;
            origin[axis] -= 1;
        }
        Self {
            origin,
            blocks,
            anchor,
            background,
            signal,
            floor: intensity.span() * MINIMUM_RANGE,
        }
    }

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

fn window_median(values: &[f32], blocks: [usize; 3]) -> Vec<f32> {
    let mut filtered = vec![0.0f32; values.len()];
    let mut window = [0.0f32; WINDOW_VOXELS];
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                let mut filled = 0;
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let neighbour = [
                                (x + dx).clamp(0, blocks[0] as i64 - 1) as usize,
                                (y + dy).clamp(0, blocks[1] as i64 - 1) as usize,
                                (z + dz).clamp(0, blocks[2] as i64 - 1) as usize,
                            ];
                            window[filled] = values[neighbour[0]
                                + blocks[0] * (neighbour[1] + blocks[1] * neighbour[2])];
                            filled += 1;
                        }
                    }
                }
                window.sort_unstable_by(f32::total_cmp);
                let slot = x as usize + blocks[0] * (y as usize + blocks[1] * z as usize);
                filtered[slot] = window[WINDOW_VOXELS / 2];
            }
        }
    }
    filtered
}

fn window_max(values: &[f32], blocks: [usize; 3]) -> Vec<f32> {
    let mut filtered = vec![0.0f32; values.len()];
    for z in 0..blocks[2] as i64 {
        for y in 0..blocks[1] as i64 {
            for x in 0..blocks[0] as i64 {
                let mut best = f32::NEG_INFINITY;
                for dz in -WINDOW..=WINDOW {
                    for dy in -WINDOW..=WINDOW {
                        for dx in -WINDOW..=WINDOW {
                            let neighbour = [
                                (x + dx).clamp(0, blocks[0] as i64 - 1) as usize,
                                (y + dy).clamp(0, blocks[1] as i64 - 1) as usize,
                                (z + dz).clamp(0, blocks[2] as i64 - 1) as usize,
                            ];
                            best = best.max(
                                values[neighbour[0]
                                    + blocks[0] * (neighbour[1] + blocks[1] * neighbour[2])],
                            );
                        }
                    }
                }
                let slot = x as usize + blocks[0] * (y as usize + blocks[1] * z as usize);
                filtered[slot] = best;
            }
        }
    }
    filtered
}

fn extrapolate(values: Vec<f32>, blocks: [usize; 3], axis: usize) -> Vec<f32> {
    let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut dims = blocks;
    dims[axis] += 2;
    let mut padded = vec![0.0f32; dims[0] * dims[1] * dims[2]];
    let mut line = Vec::with_capacity(blocks[axis]);
    for position in 0..blocks[second] {
        for row in 0..blocks[first] {
            line.clear();
            for step in 0..blocks[axis] {
                let mut index = [0usize; 3];
                index[first] = row;
                index[second] = position;
                index[axis] = step;
                line.push(values[index[0] + blocks[0] * (index[1] + blocks[1] * index[2])]);
            }
            let (forward, backward) = slope(&line);
            for (step, value) in line.iter().enumerate() {
                let mut index = [0usize; 3];
                index[first] = row;
                index[second] = position;
                index[axis] = step + 1;
                padded[index[0] + dims[0] * (index[1] + dims[1] * index[2])] = *value;
            }
            let mut index = [0usize; 3];
            index[first] = row;
            index[second] = position;
            index[axis] = 0;
            padded[index[0] + dims[0] * (index[1] + dims[1] * index[2])] = line[0] - forward;
            index[axis] = blocks[axis] + 1;
            padded[index[0] + dims[0] * (index[1] + dims[1] * index[2])] =
                line[blocks[axis] - 1] + backward;
        }
    }
    padded
}

fn slope(line: &[f32]) -> (f32, f32) {
    if line.len() < 2 {
        return (0.0, 0.0);
    }
    let first = line[1] - line[0];
    let last = line[line.len() - 1] - line[line.len() - 2];
    (first, last)
}
