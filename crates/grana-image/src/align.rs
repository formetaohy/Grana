use std::ops::Range;

use glam::Vec3;
use grana_volume::{Filter, Level, VoxelData, VoxelFormat, VoxelSemantics, VoxelValue};
use grana_warp::Warp;

pub(crate) enum Plan {
    Shift(ShiftPlan),
    Field(FieldPlan),
}

pub(crate) struct ShiftPlan {
    source: [Range<u32>; 3],
    axes: [Axis; 3],
}

pub(crate) struct FieldPlan {
    source: [Range<u32>; 3],
    level: Level,
    warp: Warp,
}

struct Axis {
    lower: Vec<Option<usize>>,
    upper: Vec<Option<usize>>,
    lower_weight: f32,
    upper_weight: f32,
}

impl Axis {
    fn tap(&self, index: usize) -> impl Iterator<Item = (usize, f32)> {
        let lower = self.lower[index].map(|at| (at, self.lower_weight));
        let upper = self.upper[index].map(|at| (at, self.upper_weight));
        lower.into_iter().chain(upper)
    }

    fn nearest(&self, index: usize) -> Option<usize> {
        match (self.lower[index], self.upper[index]) {
            (Some(lower), Some(upper)) => Some(if self.upper_weight > self.lower_weight {
                upper
            } else {
                lower
            }),
            (lower, upper) => lower.or(upper),
        }
    }
}

impl Plan {
    pub(crate) fn source(&self) -> &[Range<u32>; 3] {
        match self {
            Self::Shift(plan) => &plan.source,
            Self::Field(plan) => &plan.source,
        }
    }
}

pub(crate) fn plan(ranges: &[Range<u32>; 3], level: Level, warp: &Warp, limits: [u32; 3]) -> Plan {
    match warp.field_grid() {
        None => {
            let delta = level.basis().inverse() * Vec3::from(warp.translation());
            Plan::Shift(shift_plan(ranges, delta.to_array(), limits))
        }
        Some(_) => Plan::Field(FieldPlan {
            source: field_source(ranges, level, warp, limits),
            level,
            warp: warp.clone(),
        }),
    }
}

fn shift_plan(ranges: &[Range<u32>; 3], shift: [f32; 3], limits: [u32; 3]) -> ShiftPlan {
    let mut source = [0..0, 0..0, 0..0];
    let axes = std::array::from_fn(|axis| {
        let size = i64::from(ranges[axis].end - ranges[axis].start);
        let base = i64::from(ranges[axis].start) + shift[axis].floor() as i64;
        let limit = i64::from(limits[axis]);
        let low = base.clamp(0, limit);
        let high = (base + size + 1).clamp(0, limit);
        source[axis] = low as u32..high as u32;
        let start = low;
        let fraction = shift[axis] - shift[axis].floor();
        let mut lower = Vec::with_capacity(size as usize);
        let mut upper = Vec::with_capacity(size as usize);
        for index in 0..size {
            let at = base + index;
            lower.push((0..limit).contains(&at).then(|| (at - start) as usize));
            let next = at + 1;
            upper.push(
                (fraction > 0.0 && (0..limit).contains(&next)).then(|| (next - start) as usize),
            );
        }
        Axis {
            lower,
            upper,
            lower_weight: 1.0 - fraction,
            upper_weight: fraction,
        }
    });
    ShiftPlan { source, axes }
}

fn field_source(
    ranges: &[Range<u32>; 3],
    level: Level,
    warp: &Warp,
    limits: [u32; 3],
) -> [Range<u32>; 3] {
    let inverse = level.basis().inverse();
    let magnitude = warp.magnitude();
    [0, 1, 2].map(|axis| {
        let row = inverse.row(axis);
        let reach = magnitude * (row.x.abs() + row.y.abs() + row.z.abs());
        let pad = reach.ceil() as i64 + 1;
        let start = (i64::from(ranges[axis].start) - pad).clamp(0, i64::from(limits[axis]));
        let end = (i64::from(ranges[axis].end) + pad).clamp(0, i64::from(limits[axis]));
        start as u32..(end.max(start) as u32)
    })
}

pub(crate) fn resample(
    data: &VoxelData,
    plan: &Plan,
    ranges: &[Range<u32>; 3],
    semantics: VoxelSemantics,
) -> VoxelData {
    let nearest = !semantics.averages();
    macro_rules! resample {
        ($($variant:ident),*) => {
            match (data, plan) {
                $(
                    (VoxelData::$variant(values), Plan::Shift(plan)) => {
                        VoxelData::$variant(if nearest {
                            nearest_shift(values, plan, ranges)
                        } else {
                            interpolate_shift(values, plan, ranges)
                        })
                    }
                    (VoxelData::$variant(values), Plan::Field(plan)) => {
                        VoxelData::$variant(interpolate_field(values, plan, ranges, nearest))
                    }
                )*
            }
        };
    }
    resample!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}

pub(crate) fn zeros(format: VoxelFormat, ranges: &[Range<u32>; 3]) -> VoxelData {
    let count = [0, 1, 2]
        .iter()
        .map(|axis| (ranges[*axis].end - ranges[*axis].start) as usize)
        .product();
    VoxelData::blank(format, count)
}

fn nearest_shift<T: VoxelValue>(data: &[T], plan: &ShiftPlan, ranges: &[Range<u32>; 3]) -> Vec<T> {
    let [x_axis, y_axis, z_axis] = &plan.axes;
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let depth = (ranges[2].end - ranges[2].start) as usize;
    let source = &plan.source;
    let dims = [0, 1, 2].map(|axis| (source[axis].end - source[axis].start) as usize);
    let stride = [1, dims[0], dims[0] * dims[1]];
    assert_eq!(
        data.len(),
        dims[0] * dims[1] * dims[2],
        "the read holds every voxel of the aligned source"
    );
    let mut values = Vec::with_capacity(width * height * depth);
    for z in 0..depth {
        let Some(z_at) = z_axis.nearest(z) else {
            values.extend((0..width * height).map(|_| T::from_f32(0.0)));
            continue;
        };
        for y in 0..height {
            let Some(y_at) = y_axis.nearest(y) else {
                values.extend((0..width).map(|_| T::from_f32(0.0)));
                continue;
            };
            let base = z_at * stride[2] + y_at * stride[1];
            for x in 0..width {
                let value = x_axis.nearest(x).map_or(0.0, |at| data[base + at].to_f32());
                values.push(T::from_f32(value));
            }
        }
    }
    values
}

fn interpolate_shift<T: VoxelValue>(
    data: &[T],
    plan: &ShiftPlan,
    ranges: &[Range<u32>; 3],
) -> Vec<T> {
    let [x_axis, y_axis, z_axis] = &plan.axes;
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let depth = (ranges[2].end - ranges[2].start) as usize;
    let source = &plan.source;
    let dims = [0, 1, 2].map(|axis| (source[axis].end - source[axis].start) as usize);
    let stride = [1, dims[0], dims[0] * dims[1]];
    assert_eq!(
        data.len(),
        dims[0] * dims[1] * dims[2],
        "the read holds every voxel of the aligned source"
    );
    let mut values = Vec::with_capacity(width * height * depth);
    let mut row = vec![0.0f32; width];
    for z in 0..depth {
        for y in 0..height {
            row.fill(0.0);
            for (z_at, z_weight) in z_axis.tap(z) {
                for (y_at, y_weight) in y_axis.tap(y) {
                    let base = z_at * stride[2] + y_at * stride[1];
                    let weight = z_weight * y_weight;
                    for (x, slot) in row.iter_mut().enumerate() {
                        let mut value = 0.0;
                        if let Some(at) = x_axis.lower[x] {
                            value += data[base + at].to_f32() * x_axis.lower_weight;
                        }
                        if let Some(at) = x_axis.upper[x] {
                            value += data[base + at].to_f32() * x_axis.upper_weight;
                        }
                        *slot += value * weight;
                    }
                }
            }
            values.extend(row.iter().map(|value| T::from_f32(*value)));
        }
    }
    values
}

fn interpolate_field<T: VoxelValue>(
    data: &[T],
    plan: &FieldPlan,
    ranges: &[Range<u32>; 3],
    nearest: bool,
) -> Vec<T> {
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let depth = (ranges[2].end - ranges[2].start) as usize;
    let source = &plan.source;
    let dims = [0, 1, 2].map(|axis| (source[axis].end - source[axis].start) as usize);
    assert_eq!(
        data.len(),
        dims[0] * dims[1] * dims[2],
        "the read holds every voxel of the warped source"
    );
    let origin = Vec3::from(plan.level.origin());
    let basis = plan.level.basis();
    let matrix = basis.matrix();
    let inverse = basis.inverse();
    let start = Vec3::new(
        ranges[0].start as f32,
        ranges[1].start as f32,
        ranges[2].start as f32,
    );
    let anchor = origin + matrix * (start + Vec3::splat(0.5));
    let step = [matrix.col(0), matrix.col(1), matrix.col(2)];
    let first = Vec3::new(
        source[0].start as f32,
        source[1].start as f32,
        source[2].start as f32,
    );
    let filter = if nearest {
        Filter::Nearest
    } else {
        Filter::Linear
    };
    let mut values = Vec::with_capacity(width * height * depth);
    for z in 0..depth {
        for y in 0..height {
            let index = start + Vec3::new(0.0, y as f32, z as f32);
            let row = anchor + step[2] * z as f32 + step[1] * y as f32;
            for x in 0..width {
                let point = row + step[0] * x as f32;
                let at = index + Vec3::new(x as f32, 0.0, 0.0);
                let offset = inverse * Vec3::from(plan.warp.sample(point.to_array()));
                let position = (at + offset - first).to_array().map(f64::from);
                values.push(T::from_f32(
                    grana_volume::resample(data, dims, position, filter).unwrap_or(0.0),
                ));
            }
        }
    }
    values
}
