use super::mask::SolidMask;
use super::{Envelope, SurfaceBuilder};

const SCALE: u32 = 4;
const UNREACHED: f32 = 1.0e12;

pub(super) struct Distance {
    origin: [i64; 3],
    dims: [usize; 3],
    values: Vec<f32>,
    limit: f64,
    reach: f64,
    margin: f64,
}

impl Distance {
    pub(super) fn build(plan: &SurfaceBuilder, solid: &SolidMask, workers: usize) -> Self {
        let dims = plan.dims();
        let scale = SCALE as usize;
        let coarse = [0, 1, 2].map(|axis| dims[axis].div_ceil(scale));
        let padded = [coarse[0] + 2, coarse[1] + 2, coarse[2] + 2];
        let mut values = vec![0.0f32; padded[0] * padded[1] * padded[2]];
        let plane = padded[0] * padded[1];
        let workers = workers.clamp(1, crate::schedule::WORKER_LIMIT);
        planes(&mut values, plane, workers, |chunk, first| {
            for (row, plane_values) in chunk.chunks_mut(plane).enumerate() {
                let z = first + row;
                if z == 0 || z > coarse[2] {
                    continue;
                }
                for y in 0..coarse[1] {
                    for x in 0..coarse[0] {
                        if solid_cell(plan, solid, dims, scale, [x, y, z - 1]) {
                            plane_values[(x + 1) + padded[0] * (y + 1)] = UNREACHED;
                        }
                    }
                }
            }
        });
        let weights = [0, 1, 2].map(|axis| {
            let value = f64::from(plan.anisotropy[axis]);
            value * value
        });
        transform(&mut values, padded, weights, workers);
        let limit = (0..3).fold(0.0f64, |accumulated, axis| {
            accumulated + weights[axis] * (dims[axis] * dims[axis]) as f64
        }) + 1.0;
        let reach = values
            .iter()
            .map(|value| f64::from(*value))
            .fold(0.0f64, f64::max)
            .max(0.0)
            .sqrt()
            * f64::from(SCALE);
        let margin = (weights[0] + weights[1] + weights[2]).sqrt() * f64::from(SCALE);
        Self {
            origin: plan.anchor,
            dims: coarse,
            values,
            limit,
            reach,
            margin,
        }
    }

    pub(super) fn reach(&self) -> f64 {
        self.reach
    }

    pub(super) fn margin(&self) -> f64 {
        self.margin
    }

    pub(super) fn bound(&self, core: &[std::ops::Range<u32>; 3]) -> u32 {
        let scale = i64::from(SCALE);
        let mut low = [0usize; 3];
        let mut high = [0usize; 3];
        for axis in 0..3 {
            assert!(
                core[axis].start < core[axis].end,
                "a bounded tile covers a non empty range"
            );
            let first = i64::from(core[axis].start) - self.origin[axis];
            let last = i64::from(core[axis].end) - 1 - self.origin[axis];
            let span = self.dims[axis] as i64 * scale;
            if last < 0 || first >= span {
                return 0;
            }
            low[axis] = (first.max(0) / scale) as usize;
            high[axis] = (last.min(span - 1) / scale) as usize + 1;
        }
        let padded = [self.dims[0] + 2, self.dims[1] + 2, self.dims[2] + 2];
        let mut peak = 0.0f32;
        for z in low[2]..high[2] {
            for y in low[1]..high[1] {
                for x in low[0]..high[0] {
                    let index = (x + 1) + padded[0] * ((y + 1) + padded[1] * (z + 1));
                    peak = peak.max(self.values[index]);
                }
            }
        }
        let fine = f64::from(peak).sqrt() * f64::from(SCALE) + self.margin;
        fine.ceil() as u32
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

fn solid_cell(
    plan: &SurfaceBuilder,
    solid: &SolidMask,
    dims: [usize; 3],
    scale: usize,
    cell: [usize; 3],
) -> bool {
    let start = [cell[0] * scale, cell[1] * scale, cell[2] * scale];
    for axis in 0..3 {
        if start[axis] + scale > dims[axis] {
            return false;
        }
    }
    for dz in 0..scale {
        for dy in 0..scale {
            for dx in 0..scale {
                let voxel = [
                    plan.anchor[0] + (start[0] + dx) as i64,
                    plan.anchor[1] + (start[1] + dy) as i64,
                    plan.anchor[2] + (start[2] + dz) as i64,
                ];
                if !solid.get(voxel, plan.anchor) {
                    return false;
                }
            }
        }
    }
    true
}

fn transform(values: &mut [f32], dims: [usize; 3], weights: [f64; 3], workers: usize) {
    let plane = dims[0] * dims[1];
    if dims[0] > 1 {
        planes(values, plane, workers, |chunk, _| {
            let mut line = vec![0.0f64; dims[0]];
            let mut envelope = Envelope::new();
            for plane_values in chunk.chunks_mut(plane) {
                for y in 0..dims[1] {
                    let base = y * dims[0];
                    for (slot, value) in line.iter_mut().zip(&plane_values[base..base + dims[0]]) {
                        *slot = f64::from(*value);
                    }
                    super::parabola(&mut line, weights[0], &mut envelope);
                    for (value, slot) in line.iter().zip(&mut plane_values[base..base + dims[0]]) {
                        *slot = *value as f32;
                    }
                }
            }
        });
    }
    if dims[1] > 1 {
        planes(values, plane, workers, |chunk, _| {
            let mut line = vec![0.0f64; dims[1]];
            let mut envelope = Envelope::new();
            for plane_values in chunk.chunks_mut(plane) {
                for x in 0..dims[0] {
                    for (step, slot) in line.iter_mut().enumerate() {
                        *slot = f64::from(plane_values[x + step * dims[0]]);
                    }
                    super::parabola(&mut line, weights[1], &mut envelope);
                    for (step, value) in line.iter().enumerate() {
                        plane_values[x + step * dims[0]] = *value as f32;
                    }
                }
            }
        });
    }
    if dims[2] > 1 {
        let mut line = vec![0.0f64; dims[2]];
        let mut envelope = Envelope::new();
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let base = x + dims[0] * y;
                for (step, slot) in line.iter_mut().enumerate() {
                    *slot = f64::from(values[base + step * plane]);
                }
                super::parabola(&mut line, weights[2], &mut envelope);
                for (step, value) in line.iter().enumerate() {
                    values[base + step * plane] = *value as f32;
                }
            }
        }
    }
}

fn planes(
    values: &mut [f32],
    plane: usize,
    workers: usize,
    body: impl Fn(&mut [f32], usize) + Sync,
) {
    if plane == 0 || values.is_empty() {
        return;
    }
    let available = values.len().div_ceil(plane);
    let workers = workers.clamp(1, available.max(1));
    let each = available.div_ceil(workers).max(1);
    if workers == 1 {
        body(values, 0);
        return;
    }
    crate::schedule::parallel(|scope| {
        let body = &body;
        for (index, chunk) in values.chunks_mut(each * plane).enumerate() {
            let first = index * each;
            scope.spawn(move |_| body(chunk, first));
        }
    });
}
