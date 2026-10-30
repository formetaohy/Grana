use std::ops::Range;

use grana_object::Spot;
use grana_volume::Level;

use crate::SpotParams;

const FWHM_PER_SIGMA: f32 = 2.354_82;
const MIN_SIGMA: f32 = 0.5;
const CORE_MIN: u32 = 64;
const CORE_MAX: u32 = 256;
const CORE_STEP: u32 = 32;
const CORE_VOXELS_PER_SIGMA: f32 = 8.0;
const MAX_READ_VOXELS: u64 = 16_777_216;

type Region = [[usize; 2]; 3];

pub struct SpotScan {
    extent: [u32; 3],
    spacing: [f32; 3],
    world: [f32; 3],
    sigma_world: f32,
    truncate: [usize; 3],
    halo: [u32; 3],
    core_side: u32,
    counts: [u32; 3],
    weights: [f32; 3],
    kernels: [Vec<f32>; 3],
    threshold: f32,
    span: f32,
    radius: f32,
    source: Vec<f32>,
    blurred: Vec<f32>,
}

impl SpotScan {
    pub fn new(level: Level, span: f32, params: SpotParams) -> Self {
        assert!(
            span.is_finite() && span > 0.0,
            "a spot scan needs a positive intensity span"
        );
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let sigma_world = params.diameter() / FWHM_PER_SIGMA;
        let sigma = spacing.map(|step| (sigma_world / step).max(MIN_SIGMA));
        let truncate = sigma.map(|value| (3.0 * value).ceil() as usize);
        let halo = truncate.map(|value| value as u32 + 2);
        let maximum = sigma.iter().copied().fold(0.0f32, f32::max);
        let core_side = ((CORE_VOXELS_PER_SIGMA * maximum).ceil() as u32)
            .clamp(CORE_MIN, CORE_MAX)
            .div_ceil(CORE_STEP)
            * CORE_STEP;
        let counts = extent.map(|length| length.div_ceil(core_side));
        let maximum_read = (0..3)
            .map(|axis| u64::from(extent[axis].min(core_side) + 2 * halo[axis]))
            .product::<u64>();
        assert!(
            maximum_read <= MAX_READ_VOXELS,
            "the spot diameter needs more than {MAX_READ_VOXELS} voxels per block"
        );
        let kernels = [0, 1, 2].map(|axis| gaussian(sigma[axis], truncate[axis]));
        let weights = spacing.map(|step| 1.0 / (step * step));
        Self {
            extent,
            spacing,
            world: level.world_extent(),
            sigma_world,
            truncate,
            halo,
            core_side,
            counts,
            weights,
            kernels,
            threshold: params.threshold(),
            span,
            radius: 0.5 * params.diameter(),
            source: Vec::new(),
            blurred: Vec::new(),
        }
    }

    pub fn block_count(&self) -> usize {
        self.counts[0] as usize * self.counts[1] as usize * self.counts[2] as usize
    }

    pub fn read_range(&self, index: usize) -> [Range<u32>; 3] {
        let core = self.core(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start as i64 - i64::from(self.halo[axis]);
            let end = core[axis].end as i64 + i64::from(self.halo[axis]);
            start.max(0) as u32..end.min(i64::from(self.extent[axis])) as u32
        })
    }

    pub fn detect(&mut self, index: usize, values: &[f32]) -> Vec<Spot> {
        let core = self.core(index);
        let read = self.read_range(index);
        let read_dims = dimensions(&read);
        assert_eq!(
            values.len(),
            read_dims[0] * read_dims[1] * read_dims[2],
            "the block values cover the requested region"
        );
        let desired = [
            (core[0].end - core[0].start) as usize + 2 * self.halo[0] as usize,
            (core[1].end - core[1].start) as usize + 2 * self.halo[1] as usize,
            (core[2].end - core[2].start) as usize + 2 * self.halo[2] as usize,
        ];
        let desired_count = desired[0] * desired[1] * desired[2];
        self.source.clear();
        self.source.resize(desired_count, 0.0);
        self.blurred.resize(desired_count, 0.0);
        let origin = [
            core[0].start as i64 - i64::from(self.halo[0]),
            core[1].start as i64 - i64::from(self.halo[1]),
            core[2].start as i64 - i64::from(self.halo[2]),
        ];
        for z in 0..desired[2] {
            let volume_z = (origin[2] + z as i64).clamp(0, i64::from(self.extent[2]) - 1);
            let read_z = (volume_z - i64::from(read[2].start)) as usize;
            for y in 0..desired[1] {
                let volume_y = (origin[1] + y as i64).clamp(0, i64::from(self.extent[1]) - 1);
                let read_y = (volume_y - i64::from(read[1].start)) as usize;
                for x in 0..desired[0] {
                    let volume_x = (origin[0] + x as i64).clamp(0, i64::from(self.extent[0]) - 1);
                    let read_x = (volume_x - i64::from(read[0].start)) as usize;
                    self.source[x + desired[0] * (y + desired[1] * z)] =
                        values[read_x + read_dims[0] * (read_y + read_dims[1] * read_z)];
                }
            }
        }
        let valid = [0, 1, 2].map(|axis| [0, desired[axis]]);
        let valid_x = shrink(&valid, 0, self.truncate[0]);
        blur_axis(
            &self.source,
            &mut self.blurred,
            &desired,
            &valid_x,
            0,
            &self.kernels[0],
        );
        let valid_y = shrink(&valid_x, 1, self.truncate[1]);
        blur_axis(
            &self.blurred,
            &mut self.source,
            &desired,
            &valid_y,
            1,
            &self.kernels[1],
        );
        let valid_z = shrink(&valid_y, 2, self.truncate[2]);
        blur_axis(
            &self.source,
            &mut self.blurred,
            &desired,
            &valid_z,
            2,
            &self.kernels[2],
        );
        let response = shrink(&shrink(&shrink(&valid_z, 0, 1), 1, 1), 2, 1);
        let strides = [1usize, desired[0], desired[0] * desired[1]];
        for z in response[2][0]..response[2][1] {
            for y in response[1][0]..response[1][1] {
                let row = desired[0] * (y + desired[1] * z);
                for x in response[0][0]..response[0][1] {
                    let cell = x + row;
                    let center = self.blurred[cell];
                    let mut laplacian = 0.0;
                    for (weight, stride) in self.weights.iter().zip(strides) {
                        laplacian += weight
                            * (self.blurred[cell + stride] - 2.0 * center
                                + self.blurred[cell - stride]);
                    }
                    self.source[cell] =
                        -self.sigma_world * self.sigma_world * laplacian / self.span;
                }
            }
        }
        let mut spots = Vec::new();
        for z in core[2].clone() {
            for y in core[1].clone() {
                for x in core[0].clone() {
                    let base = x as usize - core[0].start as usize
                        + self.halo[0] as usize
                        + desired[0]
                            * (y as usize - core[1].start as usize
                                + self.halo[1] as usize
                                + desired[1]
                                    * (z as usize - core[2].start as usize
                                        + self.halo[2] as usize));
                    let value = self.source[base];
                    if value <= self.threshold {
                        continue;
                    }
                    let mut best = f32::NEG_INFINITY;
                    let mut best_index = base;
                    for dz in -1i64..=1 {
                        for dy in -1i64..=1 {
                            for dx in -1i64..=1 {
                                let neighbour = (base as i64
                                    + dx * strides[0] as i64
                                    + dy * strides[1] as i64
                                    + dz * strides[2] as i64)
                                    as usize;
                                let candidate = self.source[neighbour];
                                if candidate > best {
                                    best = candidate;
                                    best_index = neighbour;
                                }
                            }
                        }
                    }
                    if best_index != base {
                        continue;
                    }
                    let mut voxel = [x as f32, y as f32, z as f32];
                    for axis in 0..3 {
                        let step = strides[axis];
                        let minus = self.source[base - step];
                        let plus = self.source[base + step];
                        let denominator = minus - 2.0 * value + plus;
                        if denominator < -1.0e-12 {
                            voxel[axis] += (0.5 * (minus - plus) / denominator).clamp(-0.5, 0.5);
                        }
                    }
                    let center = [0, 1, 2].map(|axis| {
                        (voxel[axis] + 0.5) * self.spacing[axis] - 0.5 * self.world[axis]
                    });
                    spots.push(Spot::new(center, self.radius, value));
                }
            }
        }
        spots
    }

    fn core(&self, index: usize) -> [Range<u32>; 3] {
        let x = index % self.counts[0] as usize;
        let y = (index / self.counts[0] as usize) % self.counts[1] as usize;
        let z = index / (self.counts[0] as usize * self.counts[1] as usize);
        assert!(
            z < self.counts[2] as usize,
            "the spot scan block index is out of bounds"
        );
        [0, 1, 2].map(|axis| {
            let cell = [x, y, z][axis] as u32;
            let start = cell * self.core_side;
            start..(start + self.core_side).min(self.extent[axis])
        })
    }
}

fn dimensions(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ]
}

fn gaussian(sigma: f32, truncate: usize) -> Vec<f32> {
    let mut kernel = (0..=2 * truncate)
        .map(|index| {
            let offset = index as f32 - truncate as f32;
            (-0.5 * (offset / sigma) * (offset / sigma)).exp()
        })
        .collect::<Vec<_>>();
    let total: f32 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    kernel
}

fn shrink(region: &Region, axis: usize, amount: usize) -> Region {
    let mut shrunk = *region;
    shrunk[axis][0] += amount;
    shrunk[axis][1] -= amount;
    shrunk
}

fn blur_axis(
    input: &[f32],
    output: &mut [f32],
    dims: &[usize; 3],
    valid: &Region,
    axis: usize,
    kernel: &[f32],
) {
    let strides = [1usize, dims[0], dims[0] * dims[1]];
    let step = strides[axis] as isize;
    let radius = (kernel.len() / 2) as isize;
    for z in valid[2][0]..valid[2][1] {
        for y in valid[1][0]..valid[1][1] {
            let row = dims[0] * (y + dims[1] * z);
            for x in valid[0][0]..valid[0][1] {
                let index = x + row;
                let mut sum = 0.0;
                for (tap, weight) in kernel.iter().enumerate() {
                    sum +=
                        input[(index as isize + (tap as isize - radius) * step) as usize] * weight;
                }
                output[index] = sum;
            }
        }
    }
}
