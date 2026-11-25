use glam::Vec3;
use grana_volume::{Region, VoxelData};

use super::Grid;
use super::neighbour;
use crate::local::{LocalField, RowKey};
use grana_object::Mask;

const SCALE: f32 = 16.0;
const MINIMUM_RADIUS: f32 = 0.5;

type Chamfer = ([isize; 3], u16);

pub(super) fn radius(distance: u16, unit: f32) -> f32 {
    ((f32::from(distance) / SCALE) - 0.5).max(MINIMUM_RADIUS) * unit
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
        VoxelData::U32(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::U64(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::I8(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::I16(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::I32(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::I64(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::F32(values) => fill(values, grid, field, threshold, gate, mask),
        VoxelData::F64(values) => fill(values, grid, field, threshold, gate, mask),
    }
}

pub(super) fn distance(mask: &[u8], grid: &Grid, distance: &mut [u16]) {
    for (index, value) in mask.iter().enumerate() {
        distance[index] = if *value != 0 { u16::MAX } else { 0 };
    }
    let weights = weights(grid, grid.unit());
    let forward = weights.map(|(offset, weight)| (offset.map(|value| -value), weight));
    sweep(mask, grid, distance, &forward, false);
    sweep(mask, grid, distance, &weights, true);
}

fn weights(grid: &Grid, unit: f32) -> [Chamfer; 13] {
    neighbour::HALF.map(|position| {
        let offset = neighbour::OFFSET[position];
        let length = (0..3)
            .map(|axis| (offset[axis] as f32 * grid.spacing[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        (offset, (SCALE * length / unit).round() as u16)
    })
}

pub(super) fn nearest(tube: &[u8], grid: &Grid, owner: &mut [u32], reach: &mut [u32]) {
    let weights = weights(grid, grid.unit());
    let forward = weights.map(|(offset, weight)| (offset.map(|value| -value), weight));
    sweep_owner(tube, grid, owner, reach, &forward, false);
    sweep_owner(tube, grid, owner, reach, &weights, true);
}

fn sweep_owner(
    tube: &[u8],
    grid: &Grid,
    owner: &mut [u32],
    reach: &mut [u32],
    offsets: &[Chamfer; 13],
    reverse: bool,
) {
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
                if tube[index] == 0 {
                    continue;
                }
                let mut best = reach[index];
                let mut label = owner[index];
                for (offset, weight) in offsets {
                    let target = (index as isize
                        + offset[0] * stride[0] as isize
                        + offset[1] * stride[1] as isize
                        + offset[2] * stride[2] as isize) as usize;
                    if tube[target] == 0 {
                        continue;
                    }
                    let candidate = reach[target].saturating_add(u32::from(*weight));
                    if candidate < best {
                        best = candidate;
                        label = owner[target];
                    }
                }
                reach[index] = best;
                owner[index] = label;
            }
        }
    }
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
                for voxel_z in low_z..high_z {
                    let base = width * (voxel_z * height);
                    for voxel_y in low_y..high_y {
                        let offset = base + width * voxel_y;
                        let mut field_row =
                            field.row(RowKey::of([low_x as i64, voxel_y as i64, voxel_z as i64]));
                        for (index, value) in
                            values[offset + low_x..offset + high_x].iter().enumerate()
                        {
                            best =
                                best.max(field_row.contrast((low_x + index) as i64, value.value()));
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
                let mut best = distance[index];
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

trait Sample: Copy {
    fn value(self) -> f32;
}

macro_rules! sample {
    ($type:ty) => {
        impl Sample for $type {
            fn value(self) -> f32 {
                self as f32
            }
        }
    };
}

sample!(u8);
sample!(u16);
sample!(u32);
sample!(u64);
sample!(i8);
sample!(i16);
sample!(i32);
sample!(i64);

impl Sample for f32 {
    fn value(self) -> f32 {
        self
    }
}

impl Sample for f64 {
    fn value(self) -> f32 {
        self as f32
    }
}
