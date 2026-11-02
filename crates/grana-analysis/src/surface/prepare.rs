use super::{OUTSIDE, REMOVED, SurfaceBuilder};
use crate::local::LocalField;
use crate::mask::Mask;
use crate::morphology;

impl SurfaceBuilder {
    pub(super) fn smooth(&mut self) {
        let dims = self.dims;
        let strides = super::strides_of(dims);
        for axis in 0..3 {
            let kernel = &self.kernels[axis];
            if kernel.len() <= 1 {
                continue;
            }
            let radius = (kernel.len() / 2) as i64;
            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
            let length = dims[axis];
            let mut line = vec![0.0f32; length];
            let mut blurred = vec![0.0f32; length];
            for position in 0..dims[second] {
                for row in 0..dims[first] {
                    let base = strides[first] * row + strides[second] * position;
                    for (step, slot) in line.iter_mut().enumerate() {
                        *slot = self.raw[base + strides[axis] * step];
                    }
                    for (step, slot) in blurred.iter_mut().enumerate() {
                        let mut sum = 0.0;
                        for (tap, weight) in kernel.iter().enumerate() {
                            let sample = (step as i64 + tap as i64 - radius)
                                .clamp(0, length as i64 - 1)
                                as usize;
                            sum += line[sample] * weight;
                        }
                        *slot = sum;
                    }
                    for (step, value) in blurred.iter().enumerate() {
                        self.raw[base + strides[axis] * step] = *value;
                    }
                }
            }
        }
    }

    pub(super) fn prepare(&mut self, roi: Option<&Mask>) {
        let dims = self.dims;
        let count = dims[0] * dims[1] * dims[2];
        let start = self.start;
        let gate = Gate {
            region: [
                self.region[0].start,
                self.region[1].start,
                self.region[2].start,
            ],
            limit: [self.region[0].end, self.region[1].end, self.region[2].end],
            spacing: self.spacing,
            world: self.world,
            roi,
        };
        let floor = self.intensity.min();
        let local = LocalField::new(dims, start, self.anchor, self.intensity, |index| {
            let x = index % dims[0];
            let y = (index / dims[0]) % dims[1];
            let z = index / (dims[0] * dims[1]);
            let voxel = [
                start[0] + x as i64,
                start[1] + y as i64,
                start[2] + z as i64,
            ];
            if gate.admits(voxel) {
                self.raw[index]
            } else {
                floor
            }
        });
        self.field.clear();
        self.field.resize(count, OUTSIDE);
        let threshold = self.threshold;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                let base = dims[0] * (y + dims[1] * z);
                for x in 0..dims[0] {
                    let voxel = [
                        start[0] + x as i64,
                        start[1] + y as i64,
                        start[2] + z as i64,
                    ];
                    if !gate.admits(voxel) {
                        continue;
                    }
                    let slot = base + x;
                    let value = local.contrast(voxel, self.raw[slot]);
                    self.field[slot] = value;
                }
            }
        }

        self.mask.clear();
        self.mask.resize(count, false);
        for slot in 0..count {
            self.mask[slot] = self.field[slot] >= threshold;
        }
        let mut distance = vec![0u32; count];
        if let Some(element) = &self.opening {
            self.scratch.clear();
            self.scratch.resize(count, false);
            morphology::open(
                &mut self.mask,
                &mut self.scratch,
                &mut distance,
                dims,
                element,
            );
        }
        if let Some(element) = &self.closing {
            self.scratch.clear();
            self.scratch.resize(count, false);
            morphology::close(
                &mut self.mask,
                &mut self.scratch,
                &mut distance,
                dims,
                element,
            );
        }
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                let base = dims[0] * (y + dims[1] * z);
                for x in 0..dims[0] {
                    let voxel = [
                        start[0] + x as i64,
                        start[1] + y as i64,
                        start[2] + z as i64,
                    ];
                    let slot = base + x;
                    if !gate.admits(voxel) {
                        self.mask[slot] = false;
                        self.field[slot] = OUTSIDE;
                        continue;
                    }
                    let inside = self.field[slot] >= threshold;
                    if inside == self.mask[slot] {
                        continue;
                    }
                    self.field[slot] = if self.mask[slot] {
                        threshold
                    } else {
                        threshold - REMOVED
                    };
                }
            }
        }
    }
}

struct Gate<'a> {
    region: [u32; 3],
    limit: [u32; 3],
    spacing: [f32; 3],
    world: [f32; 3],
    roi: Option<&'a Mask>,
}

impl Gate<'_> {
    fn admits(&self, voxel: [i64; 3]) -> bool {
        let inside = (0..3).all(|axis| {
            voxel[axis] >= i64::from(self.region[axis]) && voxel[axis] < i64::from(self.limit[axis])
        });
        if !inside {
            return false;
        }
        match self.roi {
            Some(roi) => roi.contains([0, 1, 2].map(|axis| {
                (voxel[axis] as f32 + 0.5) * self.spacing[axis] - 0.5 * self.world[axis]
            })),
            None => true,
        }
    }
}
