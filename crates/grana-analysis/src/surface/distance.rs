use super::mask::SolidMask;
use super::{SurfaceBuilder, parabola};

const SCALE: u32 = 4;
const UNREACHED: f32 = 1.0e12;

pub(super) struct Distance {
    origin: [i64; 3],
    dims: [usize; 3],
    values: Vec<f32>,
    limit: f64,
    reach: f64,
    uncertainty: f64,
}

impl Distance {
    pub(super) fn build(plan: &SurfaceBuilder, solid: &SolidMask) -> Self {
        let dims = plan.dims();
        let scale = SCALE as usize;
        let coarse = [0, 1, 2].map(|axis| dims[axis].div_ceil(scale));
        let padded = [coarse[0] + 2, coarse[1] + 2, coarse[2] + 2];
        let mut values = vec![0.0f32; padded[0] * padded[1] * padded[2]];
        let mut reached = Vec::new();
        for z in 0..coarse[2] {
            for y in 0..coarse[1] {
                for x in 0..coarse[0] {
                    let voxel = [
                        plan.anchor[0] + (x * scale) as i64 + scale as i64 / 2,
                        plan.anchor[1] + (y * scale) as i64 + scale as i64 / 2,
                        plan.anchor[2] + (z * scale) as i64 + scale as i64 / 2,
                    ];
                    if solid.get(voxel, plan.anchor) {
                        let index = (x + 1) + padded[0] * ((y + 1) + padded[1] * (z + 1));
                        values[index] = UNREACHED;
                        reached.push(index);
                    }
                }
            }
        }
        let weights = [0, 1, 2].map(|axis| {
            let value = f64::from(plan.anisotropy[axis]);
            value * value
        });
        transform(&mut values, padded, weights);
        let limit = (0..3).fold(0.0f64, |accumulated, axis| {
            accumulated + weights[axis] * (dims[axis] * dims[axis]) as f64
        }) + 1.0;
        let reach = reached
            .into_iter()
            .map(|index| f64::from(values[index]))
            .fold(0.0f64, f64::max)
            .max(0.0)
            .sqrt()
            * f64::from(SCALE);
        let uncertainty = (weights[0] + weights[1] + weights[2]).sqrt() * f64::from(SCALE) * 0.5;
        Self {
            origin: plan.anchor,
            dims: coarse,
            values,
            limit,
            reach,
            uncertainty,
        }
    }

    pub(super) fn reach(&self) -> f64 {
        self.reach
    }

    pub(super) fn uncertainty(&self) -> f64 {
        self.uncertainty
    }

    pub(super) fn at(&self, voxel: [i64; 3]) -> u32 {
        let mut cell = [0usize; 3];
        for axis in 0..3 {
            let local = voxel[axis] - self.origin[axis];
            if local < 0 {
                return 0;
            }
            let index = (local / i64::from(SCALE)) as usize;
            if index >= self.dims[axis] {
                return 0;
            }
            cell[axis] = index;
        }
        let padded = [self.dims[0] + 2, self.dims[1] + 2, self.dims[2] + 2];
        let index = (cell[0] + 1) + padded[0] * ((cell[1] + 1) + padded[1] * (cell[2] + 1));
        let value = f64::from(self.values[index]) * f64::from(SCALE * SCALE);
        value.min(self.limit) as u32
    }
}

fn transform(values: &mut [f32], dims: [usize; 3], weights: [f64; 3]) {
    let strides = [1usize, dims[0], dims[0] * dims[1]];
    let longest = dims[0].max(dims[1]).max(dims[2]);
    let mut line = vec![0.0f64; longest];
    for axis in 0..3 {
        let weight = weights[axis];
        let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
        let length = dims[axis];
        for second_position in 0..dims[second] {
            for first_position in 0..dims[first] {
                let base = strides[first] * first_position + strides[second] * second_position;
                for (step, slot) in line[..length].iter_mut().enumerate() {
                    *slot = f64::from(values[base + strides[axis] * step]);
                }
                parabola(&mut line[..length], weight);
                for (step, value) in line[..length].iter().enumerate() {
                    values[base + strides[axis] * step] = *value as f32;
                }
            }
        }
    }
}
