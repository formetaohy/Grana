use glam::Vec3;
use grana_volume::{Region, VoxelData};

use super::Grid;
use crate::local::LocalField;
use grana_object::Mask;

const AXIS: u16 = 16;
const FACE: u16 = 23;
const CUBE: u16 = 27;

const SCALE: f32 = 16.0;
const MINIMUM_RADIUS: f32 = 0.5;

type Chamfer = ([isize; 3], u16);

const FORWARD: [Chamfer; 13] = forward();

const BACKWARD: [Chamfer; 13] = backward();

pub(super) fn radius(distance: u16, spacing: f32) -> f32 {
    ((f32::from(distance) / SCALE) - 0.5).max(MINIMUM_RADIUS) * spacing
}

pub(super) struct Gate<'a> {
    pub(super) region: Region,
    pub(super) roi: Option<&'a Mask>,
}

impl Gate<'_> {
    fn admits(&self, point: [f32; 3]) -> bool {
        self.region.contains(Vec3::from(point)) && self.roi.is_none_or(|roi| roi.contains(point))
    }
}

pub(super) fn mask(
    data: &VoxelData,
    grid: &Grid,
    field: &LocalField,
    threshold: f32,
    gate: Gate<'_>,
    mask: &mut [u8],
) {
    match data {
        VoxelData::U8(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::U16(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::F32(values) => fill(values, grid, field, threshold, gate, mask),
    }
}

pub(super) fn distance(mask: &[u8], grid: &Grid, distance: &mut [u16]) {
    for (index, value) in mask.iter().enumerate() {
        distance[index] = if *value != 0 { u16::MAX } else { 0 };
    }
    sweep(mask, grid, distance, &FORWARD, false);
    sweep(mask, grid, distance, &BACKWARD, true);
}

fn fill<T: Sample>(
    values: &[T],
    grid: &Grid,
    field: &LocalField,
    threshold: f32,
    gate: Gate<'_>,
    mask: &mut [u8],
) {
    let extent = grid.extent;
    let step = grid.step;
    let width = extent[0] as usize;
    let height = extent[1] as usize;
    let depth = extent[2] as usize;
    let stride = grid.stride;
    for z in 0..grid.dims[2] as usize {
        let low_z = z * step[2] as usize;
        let high_z = (low_z + step[2] as usize).min(depth);
        for y in 0..grid.dims[1] as usize {
            let low_y = y * step[1] as usize;
            let high_y = (low_y + step[1] as usize).min(height);
            for x in 0..grid.dims[0] as usize {
                let cell = stride[2] * (z + 1) + stride[1] * (y + 1) + x + 1;
                if !gate.admits(grid.position([x as f32, y as f32, z as f32])) {
                    mask[cell] = 0;
                    continue;
                }
                let low_x = x * step[0] as usize;
                let high_x = (low_x + step[0] as usize).min(width);
                let mut best = f32::NEG_INFINITY;
                for row in low_z..high_z {
                    let base = width * (row * height);
                    for column in low_y..high_y {
                        let offset = base + width * column;
                        for (index, value) in
                            values[offset + low_x..offset + high_x].iter().enumerate()
                        {
                            let voxel = [(low_x + index) as i64, column as i64, row as i64];
                            best = best.max(field.contrast(voxel, value.value()));
                        }
                    }
                }
                mask[cell] = u8::from(best > threshold);
            }
        }
    }
}

fn sweep(mask: &[u8], grid: &Grid, distance: &mut [u16], offsets: &[Chamfer; 13], reverse: bool) {
    let dims = grid.dims;
    let stride = grid.stride;
    for step_z in 0..dims[2] {
        let z = if reverse {
            dims[2] - 1 - step_z
        } else {
            step_z
        };
        for step_y in 0..dims[1] {
            let y = if reverse {
                dims[1] - 1 - step_y
            } else {
                step_y
            };
            for step_x in 0..dims[0] {
                let x = if reverse {
                    dims[0] - 1 - step_x
                } else {
                    step_x
                };
                let index =
                    stride[2] * (z as usize + 1) + stride[1] * (y as usize + 1) + x as usize + 1;
                if mask[index] == 0 {
                    continue;
                }
                let mut best = u16::MAX;
                for (offset, weight) in offsets {
                    let target = (index as isize
                        + offset[0] * stride[0] as isize
                        + offset[1] * stride[1] as isize
                        + offset[2] * stride[2] as isize) as usize;
                    best = best.min(distance[target].saturating_add(*weight));
                }
                distance[index] = best;
            }
        }
    }
}

const fn forward() -> [Chamfer; 13] {
    let mut table = [([0isize; 3], 0u16); 13];
    let mut found = 0;
    let mut z = -1isize;
    while z <= 1 {
        let mut y = -1isize;
        while y <= 1 {
            let mut x = -1isize;
            while x <= 1 {
                if (x != 0 || y != 0 || z != 0)
                    && (z < 0 || (z == 0 && (y < 0 || (y == 0 && x < 0))))
                {
                    let mut weight = AXIS;
                    if (x != 0) as u16 + (y != 0) as u16 + (z != 0) as u16 == 2 {
                        weight = FACE;
                    } else if x != 0 && y != 0 && z != 0 {
                        weight = CUBE;
                    }
                    table[found] = ([x, y, z], weight);
                    found += 1;
                }
                x += 1;
            }
            y += 1;
        }
        z += 1;
    }
    table
}

const fn backward() -> [Chamfer; 13] {
    let forward = forward();
    let mut table = [([0isize; 3], 0u16); 13];
    let mut index = 0;
    while index < 13 {
        let offset = forward[index].0;
        table[index] = ([-offset[0], -offset[1], -offset[2]], forward[index].1);
        index += 1;
    }
    table
}

trait Sample: Copy {
    fn value(self) -> f32;
}

impl Sample for u8 {
    fn value(self) -> f32 {
        f32::from(self)
    }
}

impl Sample for u16 {
    fn value(self) -> f32 {
        f32::from(self)
    }
}

impl Sample for f32 {
    fn value(self) -> f32 {
        self
    }
}
