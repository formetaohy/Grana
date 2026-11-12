use std::sync::atomic::{AtomicBool, Ordering};

use grana_volume::Level;

use super::elevation;
use crate::schedule;

const SIGMA_LIMIT: f32 = 4.0;
const KERNEL_LIMIT: usize = 16;
const BINS: usize = 256;
const STEP: usize = (1 << 16) / BINS;
const PLATE_FLOOR: f32 = 1.0e-6;
const MAGNITUDE: f32 = 0.25;

pub(crate) struct Shell {
    pub centre: [f32; 3],
    pub inner: f32,
    pub outer: f32,
}

struct Smoothing {
    kernel: Vec<f32>,
    radius: usize,
}

impl Smoothing {
    fn new(level: Level, grain: f32) -> Option<Self> {
        if !(grain.is_finite() && grain > 0.0) {
            return None;
        }
        let sigma = sigma(level, grain);
        if sigma < 0.5 {
            return None;
        }
        let radius = ((3.0 * sigma).ceil() as usize).min(KERNEL_LIMIT);
        let mut kernel = Vec::with_capacity(2 * radius + 1);
        let mut sum = 0.0f32;
        for step in -(radius as i64)..=radius as i64 {
            let weight = (-0.5 * (step as f32 / sigma).powi(2)).exp();
            kernel.push(weight);
            sum += weight;
        }
        for weight in &mut kernel {
            *weight /= sum;
        }
        Some(Self { kernel, radius })
    }

    fn apply(&self, values: &mut [f32], dims: [usize; 3], cancel: &AtomicBool) -> bool {
        let mut scratch = vec![0.0f32; values.len()];
        for axis in 0..3 {
            self.axis(values, &mut scratch, dims, axis);
            values.copy_from_slice(&scratch);
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
        }
        true
    }

    fn axis(&self, input: &[f32], output: &mut [f32], dims: [usize; 3], axis: usize) {
        let [width, height, depth] = dims;
        let radius = self.radius as i64;
        for z in 0..depth {
            for y in 0..height {
                for x in 0..width {
                    let mut sum = 0.0f32;
                    for (index, weight) in self.kernel.iter().enumerate() {
                        let step = index as i64 - radius;
                        let at = match axis {
                            0 => (clamp(x as i64 + step, width), y, z),
                            1 => (x, clamp(y as i64 + step, height), z),
                            _ => (x, y, clamp(z as i64 + step, depth)),
                        };
                        sum += weight * input[at.0 + width * (at.1 + height * at.2)];
                    }
                    output[x + width * (y + height * z)] = sum;
                }
            }
        }
    }
}

pub(crate) fn elevations(
    values: &[f32],
    dims: [usize; 3],
    level: Level,
    grain: f32,
    workers: usize,
    cancel: &AtomicBool,
) -> Option<Vec<u16>> {
    let count = dims[0] * dims[1] * dims[2];
    assert_eq!(
        values.len(),
        count,
        "the membrane field covers every voxel of the growth box"
    );
    if dims.iter().any(|axis| *axis < 3) {
        return Some(vec![0; count]);
    }
    let mut smoothed = values.to_vec();
    if let Some(smoothing) = Smoothing::new(level, grain)
        && !smoothing.apply(&mut smoothed, dims, cancel)
    {
        return None;
    }
    let sigma = sigma(level, grain).max(0.5);
    let spacing = level.voxel();
    let narrowest = spacing[0].min(spacing[1]).min(spacing[2]);
    let weights = [
        sigma * sigma * (narrowest / spacing[0]).powi(2),
        sigma * sigma * (narrowest / spacing[1]).powi(2),
        sigma * sigma * (narrowest / spacing[2]).powi(2),
    ];
    let cross = [
        0.25 * (weights[0] * weights[1]).sqrt(),
        0.25 * (weights[0] * weights[2]).sqrt(),
        0.25 * (weights[1] * weights[2]).sqrt(),
    ];
    let plane = dims[0] * dims[1];
    let slices = (0..dims[2]).collect::<Vec<_>>();
    let computed = schedule::map(&slices, workers, |&z| {
        let mut slice = vec![0u16; plane];
        if z == 0 || z + 1 == dims[2] {
            return slice;
        }
        let at = |x: usize, y: usize, z: usize| smoothed[x + dims[0] * (y + dims[1] * z)];
        for y in 1..dims[1] - 1 {
            for x in 1..dims[0] - 1 {
                let centre = at(x, y, z);
                let xx = (at(x + 1, y, z) - 2.0 * centre + at(x - 1, y, z)) * weights[0];
                let yy = (at(x, y + 1, z) - 2.0 * centre + at(x, y - 1, z)) * weights[1];
                let zz = (at(x, y, z + 1) - 2.0 * centre + at(x, y, z - 1)) * weights[2];
                let xy = (at(x + 1, y + 1, z) - at(x + 1, y - 1, z) - at(x - 1, y + 1, z)
                    + at(x - 1, y - 1, z))
                    * cross[0];
                let xz = (at(x + 1, y, z + 1) - at(x + 1, y, z - 1) - at(x - 1, y, z + 1)
                    + at(x - 1, y, z - 1))
                    * cross[1];
                let yz = (at(x, y + 1, z + 1) - at(x, y + 1, z - 1) - at(x, y - 1, z + 1)
                    + at(x, y - 1, z - 1))
                    * cross[2];
                let hessian = [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]];
                slice[x + dims[0] * y] = elevation(plate(hessian));
            }
        }
        slice
    });
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let mut output = vec![0u16; count];
    for (z, slice) in computed.into_iter().enumerate() {
        output[z * plane..(z + 1) * plane].copy_from_slice(&slice);
    }
    Some(dilate(&output, dims, workers))
}

fn dilate(response: &[u16], dims: [usize; 3], workers: usize) -> Vec<u16> {
    let plane = dims[0] * dims[1];
    let slices = (0..dims[2]).collect::<Vec<_>>();
    let computed = schedule::map(&slices, workers, |&z| {
        let mut slice = vec![0u16; plane];
        let at = |x: usize, y: usize, z: usize| response[x + dims[0] * (y + dims[1] * z)];
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let mut best = at(x, y, z);
                for axis in 0..3 {
                    for direction in [-1i64, 1] {
                        let target = [x, y, z][axis] as i64 + direction;
                        if target < 0 || target >= dims[axis] as i64 {
                            continue;
                        }
                        let mut neighbour = [x, y, z];
                        neighbour[axis] = target as usize;
                        best = best.max(at(neighbour[0], neighbour[1], neighbour[2]));
                    }
                }
                slice[x + dims[0] * y] = best;
            }
        }
        slice
    });
    let mut output = vec![0u16; response.len()];
    for (z, slice) in computed.into_iter().enumerate() {
        output[z * plane..(z + 1) * plane].copy_from_slice(&slice);
    }
    output
}

pub(crate) fn barrier(
    elevations: &[u16],
    dims: [usize; 3],
    spacing: [f32; 3],
    shells: &[Shell],
) -> u16 {
    let mut histogram = [0u64; BINS];
    let narrowest = spacing[0].min(spacing[1]).min(spacing[2]);
    for shell in shells {
        if shell.outer <= shell.inner {
            continue;
        }
        let reach = (shell.outer / narrowest).ceil() as i64;
        let base = shell.centre.map(|value| value.floor() as i64);
        let inner = shell.inner * shell.inner;
        let outer = shell.outer * shell.outer;
        for dz in -reach..=reach {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let at = [base[0] + dx, base[1] + dy, base[2] + dz];
                    if (0..3).any(|axis| at[axis] < 0 || at[axis] >= dims[axis] as i64) {
                        continue;
                    }
                    let offset = [
                        dx as f32 * spacing[0],
                        dy as f32 * spacing[1],
                        dz as f32 * spacing[2],
                    ];
                    let squared =
                        offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2];
                    if squared < inner || squared > outer {
                        continue;
                    }
                    let slot =
                        at[0] as usize + dims[0] * (at[1] as usize + dims[1] * at[2] as usize);
                    histogram[usize::from(elevations[slot]) / STEP] += 1;
                }
            }
        }
    }
    let bin = otsu(&histogram);
    match bin {
        Some(bin) => ((bin + 1) * STEP - 1) as u16,
        None => u16::MAX,
    }
}

fn otsu(histogram: &[u64; BINS]) -> Option<usize> {
    if histogram.iter().filter(|count| **count > 0).count() < 2 {
        return None;
    }
    let total = histogram.iter().sum::<u64>();
    let weighted = histogram
        .iter()
        .enumerate()
        .map(|(bin, count)| bin as u64 * count)
        .sum::<u64>();
    let mut background = 0u64;
    let mut weight = 0u64;
    let mut best = 0usize;
    let mut variance = -1.0f64;
    for (bin, count) in histogram.iter().enumerate() {
        background += count;
        if background == 0 {
            continue;
        }
        let foreground = total - background;
        if foreground == 0 {
            break;
        }
        weight += bin as u64 * count;
        let mean_background = weight as f64 / background as f64;
        let mean_foreground = (weighted - weight) as f64 / foreground as f64;
        let current =
            background as f64 * foreground as f64 * (mean_background - mean_foreground).powi(2);
        if current > variance {
            variance = current;
            best = bin;
        }
    }
    Some(best)
}

fn sigma(level: Level, grain: f32) -> f32 {
    let spacing = level.spacing();
    let narrowest = spacing.x().min(spacing.y()).min(spacing.z());
    if narrowest <= 0.0 {
        return 0.5;
    }
    (grain / narrowest).min(SIGMA_LIMIT)
}

fn plate(hessian: [[f32; 3]; 3]) -> f32 {
    let [first, second, third] = eigenvalues(hessian);
    let mut values = [first, second, third];
    values.sort_by(|left, right| right.abs().total_cmp(&left.abs()));
    let [first, second, third] = values;
    if first >= 0.0 {
        return 0.0;
    }
    let sheet = (first.abs() - second.abs()) / (first.abs() + second.abs() + PLATE_FLOOR);
    let norm = (first * first + second * second + third * third).sqrt();
    let magnitude = norm / (norm + MAGNITUDE);
    (sheet * magnitude).clamp(0.0, 1.0)
}

fn eigenvalues(matrix: [[f32; 3]; 3]) -> [f32; 3] {
    let [row_x, row_y, row_z] = matrix;
    let mean = (row_x[0] + row_y[1] + row_z[2]) / 3.0;
    let pair = row_x[1] * row_x[1] + row_x[2] * row_x[2] + row_y[2] * row_y[2];
    let spread = ((row_x[0] - mean).powi(2)
        + (row_y[1] - mean).powi(2)
        + (row_z[2] - mean).powi(2)
        + 2.0 * pair)
        / 6.0;
    let scale = spread.sqrt();
    if scale <= 1.0e-12 {
        return [mean, mean, mean];
    }
    let xx = (row_x[0] - mean) / scale;
    let yy = (row_y[1] - mean) / scale;
    let zz = (row_z[2] - mean) / scale;
    let xy = row_x[1] / scale;
    let xz = row_x[2] / scale;
    let yz = row_y[2] / scale;
    let determinant =
        xx * (yy * zz - yz * yz) - xy * (xy * zz - yz * xz) + xz * (xy * yz - yy * xz);
    let angle = (determinant / 2.0).clamp(-1.0, 1.0).acos() / 3.0;
    let large = mean + 2.0 * scale * angle.cos();
    let small = mean + 2.0 * scale * (angle + 2.0 * std::f32::consts::PI / 3.0).cos();
    [large, 3.0 * mean - large - small, small]
}

fn clamp(index: i64, length: usize) -> usize {
    index.clamp(0, length as i64 - 1) as usize
}
