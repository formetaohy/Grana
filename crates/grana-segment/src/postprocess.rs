use std::collections::VecDeque;

use rayon::prelude::*;

const INTEGRATION_STEPS: usize = 50;
const STEP_SIZE: f32 = 1.0;
const CONVERGENCE: f32 = 0.005;
const SIGMA: f64 = 0.5;
const FOREGROUND_WEIGHT: f32 = 0.9;
const TRACE_BATCH: usize = 1 << 20;
const LEVELS: usize = 65_536;

pub(crate) struct Prediction {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) depth: usize,
    pub(crate) channels: [Vec<f32>; 4],
}

impl Prediction {
    pub(crate) fn shape(&self) -> [usize; 3] {
        [self.width, self.height, self.depth]
    }
}

pub(crate) fn labels(
    prediction: &Prediction,
    foreground_threshold: f32,
    density_threshold: f32,
) -> Vec<u32> {
    let shape = prediction.shape();
    let area = shape[0] * shape[1] * shape[2];
    let foreground = &prediction.channels[0];
    let mut mask = vec![0u8; area];
    for (index, value) in foreground.iter().enumerate() {
        if *value > foreground_threshold {
            mask[index] = 1;
        }
    }
    if !mask.iter().any(|value| *value != 0) {
        return vec![0; area];
    }
    let zero = (prediction.depth == 1).then(|| vec![0f32; area]);
    let z = zero
        .as_ref()
        .map_or(prediction.channels[1].as_slice(), |zero| zero.as_slice());
    let flow = [
        prediction.channels[3].as_slice(),
        prediction.channels[2].as_slice(),
        z,
    ];
    let mut density = flow_density(flow, &mask, shape);
    density = gaussian(&density, shape);
    for (value, mask) in density.iter_mut().zip(&mask) {
        if *mask == 0 {
            *value = 0.0;
        }
    }
    let markers = components(&density, density_threshold, shape);
    drop(density);
    let heightmap = heightmap(foreground, flow);
    let mut segmentation = watershed(&heightmap, &markers, &mask, shape);
    drop(markers);
    renumber(&mut segmentation);
    segmentation
}

fn flow_density(flow: [&[f32]; 3], mask: &[u8], shape: [usize; 3]) -> Vec<f32> {
    let area = shape[0] * shape[1] * shape[2];
    let indices = (0..area)
        .filter(|index| mask[*index] != 0)
        .collect::<Vec<_>>();
    let mut density = vec![0f32; area];
    for chunk in indices.chunks(TRACE_BATCH) {
        let mut positions = chunk
            .iter()
            .map(|index| coordinate(*index, shape))
            .collect::<Vec<_>>();
        positions.par_iter_mut().for_each(|position| {
            trace(position, flow, mask, shape);
        });
        for position in positions {
            density[flat_index(position, shape)] += 1.0;
        }
    }
    density
}

fn trace(position: &mut [f32; 3], flow: [&[f32]; 3], mask: &[u8], shape: [usize; 3]) {
    let upper = [
        (shape[0] - 1) as f32,
        (shape[1] - 1) as f32,
        (shape[2] - 1) as f32,
    ];
    for _ in 0..INTEGRATION_STEPS {
        let mut step = sample(flow, shape, *position);
        let mut middle = [0f32; 3];
        for axis in 0..3 {
            middle[axis] = position[axis] + 0.5 * STEP_SIZE * step[axis];
        }
        clip(&mut middle, upper);
        step = sample(flow, shape, middle);
        let convergence = (0..3)
            .map(|axis| (STEP_SIZE * step[axis]).abs())
            .fold(0.0f32, f32::max);
        if convergence < CONVERGENCE {
            break;
        }
        let mut proposed = [0f32; 3];
        for axis in 0..3 {
            proposed[axis] = position[axis] + STEP_SIZE * step[axis];
        }
        if !inside(&proposed, upper) || mask[flat_index(proposed, shape)] == 0 {
            break;
        }
        *position = proposed;
    }
}

fn sample(flow: [&[f32]; 3], shape: [usize; 3], position: [f32; 3]) -> [f32; 3] {
    let upper = [
        (shape[0] - 1) as f32,
        (shape[1] - 1) as f32,
        (shape[2] - 1) as f32,
    ];
    let mut clamped = position;
    clip(&mut clamped, upper);
    let lower = [
        clamped[0] as usize,
        clamped[1] as usize,
        clamped[2] as usize,
    ];
    let fraction = [
        clamped[0] - lower[0] as f32,
        clamped[1] - lower[1] as f32,
        clamped[2] - lower[2] as f32,
    ];
    let next = [
        (lower[0] + 1).min(shape[0] - 1),
        (lower[1] + 1).min(shape[1] - 1),
        (lower[2] + 1).min(shape[2] - 1),
    ];
    let stride = [1usize, shape[0], shape[0] * shape[1]];
    let base = lower[0] * stride[0] + lower[1] * stride[1] + lower[2] * stride[2];
    let step_x = (next[0] - lower[0]) * stride[0];
    let step_y = (next[1] - lower[1]) * stride[1];
    let step_z = (next[2] - lower[2]) * stride[2];
    let offsets = [
        base,
        base + step_x,
        base + step_y,
        base + step_x + step_y,
        base + step_z,
        base + step_x + step_z,
        base + step_y + step_z,
        base + step_x + step_y + step_z,
    ];
    let mut result = [0f32; 3];
    for axis in 0..3 {
        let values = flow[axis];
        let c00 = values[offsets[0]] + fraction[0] * (values[offsets[1]] - values[offsets[0]]);
        let c01 = values[offsets[2]] + fraction[0] * (values[offsets[3]] - values[offsets[2]]);
        let c10 = values[offsets[4]] + fraction[0] * (values[offsets[5]] - values[offsets[4]]);
        let c11 = values[offsets[6]] + fraction[0] * (values[offsets[7]] - values[offsets[6]]);
        let c0 = c00 + fraction[1] * (c01 - c00);
        let c1 = c10 + fraction[1] * (c11 - c10);
        result[axis] = -(c0 + fraction[2] * (c1 - c0));
    }
    result
}

fn coordinate(index: usize, shape: [usize; 3]) -> [f32; 3] {
    let x = index % shape[0];
    let y = index / shape[0] % shape[1];
    let z = index / (shape[0] * shape[1]);
    [x as f32, y as f32, z as f32]
}

fn flat_index(position: [f32; 3], shape: [usize; 3]) -> usize {
    let upper = [
        (shape[0] - 1) as f32,
        (shape[1] - 1) as f32,
        (shape[2] - 1) as f32,
    ];
    let mut index = [0usize; 3];
    for axis in 0..3 {
        let value = position[axis].clamp(0.0, upper[axis]);
        index[axis] = (value + 0.5) as usize;
    }
    index[0] + shape[0] * (index[1] + shape[1] * index[2])
}

fn clip(position: &mut [f32; 3], upper: [f32; 3]) {
    for axis in 0..3 {
        position[axis] = position[axis].clamp(0.0, upper[axis]);
    }
}

fn inside(position: &[f32; 3], upper: [f32; 3]) -> bool {
    (0..3).all(|axis| position[axis] >= 0.0 && position[axis] <= upper[axis])
}

fn gaussian(values: &[f32], shape: [usize; 3]) -> Vec<f32> {
    let radius = ((3.0 * SIGMA).ceil() as usize).max(1);
    let mut kernel = vec![0f64; radius + 1];
    for (offset, value) in kernel.iter_mut().enumerate() {
        let position = offset as f64;
        *value = (-position * position / (2.0 * SIGMA * SIGMA)).exp();
    }
    let sum = kernel[0] + 2.0 * kernel[1..].iter().sum::<f64>();
    let kernel = kernel
        .iter()
        .map(|value| (*value / sum) as f32)
        .collect::<Vec<_>>();
    let mut current = values.to_vec();
    if shape[2] > 1 {
        current = convolve_axis(&current, shape, 2, &kernel);
    }
    current = convolve_axis(&current, shape, 1, &kernel);
    convolve_axis(&current, shape, 0, &kernel)
}

fn convolve_axis(values: &[f32], shape: [usize; 3], axis: usize, kernel: &[f32]) -> Vec<f32> {
    let width = shape[0];
    let height = shape[1];
    let depth = shape[2];
    let mut output = vec![0f32; values.len()];
    if axis == 0 {
        output
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(row, output)| {
                let base = row * width;
                for offset in 0..width {
                    let mut total = kernel[0] * values[base + offset];
                    for (tap, coefficient) in kernel.iter().enumerate().skip(1) {
                        let below = mirror(offset as isize - tap as isize, width);
                        let above = mirror(offset as isize + tap as isize, width);
                        total += coefficient * (values[base + below] + values[base + above]);
                    }
                    output[offset] = total;
                }
            });
        return output;
    }
    if axis == 1 {
        output
            .par_iter_mut()
            .enumerate()
            .for_each(|(index, output)| {
                let x = index % width;
                let y = index / width % height;
                let z = index / (width * height);
                let base = z * width * height + x;
                let mut total = kernel[0] * values[base + y * width];
                for (tap, coefficient) in kernel.iter().enumerate().skip(1) {
                    let below = mirror(y as isize - tap as isize, height);
                    let above = mirror(y as isize + tap as isize, height);
                    total +=
                        coefficient * (values[base + below * width] + values[base + above * width]);
                }
                *output = total;
            });
        return output;
    }
    output
        .par_iter_mut()
        .enumerate()
        .for_each(|(index, output)| {
            let x = index % width;
            let y = index / width % height;
            let z = index / (width * height);
            let base = y * width + x;
            let mut total = kernel[0] * values[base + z * width * height];
            for (tap, coefficient) in kernel.iter().enumerate().skip(1) {
                let below = mirror(z as isize - tap as isize, depth);
                let above = mirror(z as isize + tap as isize, depth);
                total += coefficient
                    * (values[base + below * width * height]
                        + values[base + above * width * height]);
            }
            *output = total;
        });
    output
}

fn mirror(index: isize, length: usize) -> usize {
    if length == 1 {
        return 0;
    }
    let length = length as isize;
    let period = 2 * (length - 1);
    let mut value = index.rem_euclid(period);
    if value >= length {
        value = period - value;
    }
    value as usize
}

fn components(values: &[f32], threshold: f32, shape: [usize; 3]) -> Vec<u32> {
    let mut labels = vec![0u32; values.len()];
    let mut queue = VecDeque::new();
    let mut next = 0u32;
    for seed in 0..values.len() {
        if values[seed] <= threshold || labels[seed] != 0 {
            continue;
        }
        next += 1;
        labels[seed] = next;
        queue.push_back(seed);
        while let Some(index) = queue.pop_front() {
            let x = index % shape[0];
            let y = index / shape[0] % shape[1];
            let z = index / (shape[0] * shape[1]);
            for offset_z in -1isize..=1 {
                let near_z = z as isize + offset_z;
                if near_z < 0 || near_z >= shape[2] as isize {
                    continue;
                }
                for offset_y in -1isize..=1 {
                    let near_y = y as isize + offset_y;
                    if near_y < 0 || near_y >= shape[1] as isize {
                        continue;
                    }
                    for offset_x in -1isize..=1 {
                        let near_x = x as isize + offset_x;
                        if near_x < 0 || near_x >= shape[0] as isize {
                            continue;
                        }
                        if offset_x == 0 && offset_y == 0 && offset_z == 0 {
                            continue;
                        }
                        let neighbour = near_x as usize
                            + shape[0] * (near_y as usize + shape[1] * near_z as usize);
                        if values[neighbour] > threshold && labels[neighbour] == 0 {
                            labels[neighbour] = next;
                            queue.push_back(neighbour);
                        }
                    }
                }
            }
        }
    }
    labels
}

fn heightmap(foreground: &[f32], flow: [&[f32]; 3]) -> Vec<f32> {
    let mut maximum = 0f32;
    let mut norms = vec![0f32; foreground.len()];
    for (index, norm) in norms.iter_mut().enumerate() {
        let z = flow[0][index];
        let y = flow[1][index];
        let x = flow[2][index];
        *norm = (z * z + y * y + x * x).sqrt();
        maximum = maximum.max(*norm);
    }
    let mut inverted_max = 0f32;
    for norm in &norms {
        inverted_max = inverted_max.max(maximum - norm);
    }
    let denominator = inverted_max + 1.0e-9;
    let mut heightmap = vec![0f32; foreground.len()];
    for (index, height) in heightmap.iter_mut().enumerate() {
        let inverted = (maximum - norms[index]) / denominator;
        *height = FOREGROUND_WEIGHT * (1.0 - foreground[index].clamp(0.0, 1.0))
            + (1.0 - FOREGROUND_WEIGHT) * inverted;
    }
    heightmap
}

fn watershed(heightmap: &[f32], markers: &[u32], mask: &[u8], shape: [usize; 3]) -> Vec<u32> {
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for (index, value) in heightmap.iter().enumerate() {
        if mask[index] == 0 {
            continue;
        }
        minimum = minimum.min(*value);
        maximum = maximum.max(*value);
    }
    let mut levels = vec![0u16; heightmap.len()];
    let range = (maximum - minimum) as f64;
    if range > 0.0 {
        let scale = (LEVELS - 1) as f64 / range;
        for (index, level) in levels.iter_mut().enumerate() {
            if mask[index] == 0 {
                continue;
            }
            let quantized =
                ((heightmap[index] - minimum) as f64 * scale).clamp(0.0, (LEVELS - 1) as f64);
            *level = quantized as u16;
        }
    }
    let mut buckets = (0..LEVELS).map(|_| Vec::new()).collect::<Vec<Vec<u64>>>();
    let mut labels = vec![0u32; heightmap.len()];
    let mut current = LEVELS;
    for (index, marker) in markers.iter().enumerate() {
        if mask[index] == 0 || *marker == 0 {
            continue;
        }
        labels[index] = *marker;
        let level = levels[index] as usize;
        buckets[level].push(index as u64);
        current = current.min(level);
    }
    while current < LEVELS {
        let mut bucket = std::mem::take(&mut buckets[current]);
        while let Some(index) = bucket.pop() {
            let index = index as usize;
            let label = labels[index];
            let x = index % shape[0];
            let y = index / shape[0] % shape[1];
            let z = index / (shape[0] * shape[1]);
            for neighbour in neighbours(x, y, z, shape).into_iter().flatten() {
                if mask[neighbour] == 0 || labels[neighbour] != 0 {
                    continue;
                }
                labels[neighbour] = label;
                let level = (levels[neighbour] as usize).max(current);
                if level == current {
                    bucket.push(neighbour as u64);
                } else {
                    buckets[level].push(neighbour as u64);
                }
            }
        }
        current += 1;
    }
    labels
}

fn neighbours(x: usize, y: usize, z: usize, shape: [usize; 3]) -> [Option<usize>; 6] {
    let stride_y = shape[0];
    let stride_z = shape[0] * shape[1];
    let index = x + stride_y * y + stride_z * z;
    [
        (x > 0).then(|| index - 1),
        (x + 1 < shape[0]).then(|| index + 1),
        (y > 0).then(|| index - stride_y),
        (y + 1 < shape[1]).then(|| index + stride_y),
        (z > 0).then(|| index - stride_z),
        (z + 1 < shape[2]).then(|| index + stride_z),
    ]
}

fn renumber(labels: &mut [u32]) {
    let mut maximum = 0u32;
    for label in labels.iter() {
        maximum = maximum.max(*label);
    }
    if maximum == 0 {
        return;
    }
    let mut mapping = vec![0u32; maximum as usize + 1];
    let mut next = 0u32;
    for label in labels.iter_mut() {
        if *label == 0 {
            continue;
        }
        let slot = &mut mapping[*label as usize];
        if *slot == 0 {
            next += 1;
            *slot = next;
        }
        *label = *slot;
    }
}
