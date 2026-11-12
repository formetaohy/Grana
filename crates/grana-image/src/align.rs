use std::ops::Range;

use glam::Vec3;
use grana_volume::{Level, VoxelData, VoxelFormat};
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
    limits: [u32; 3],
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
            limits,
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

pub(crate) fn resample(data: &VoxelData, plan: &Plan, ranges: &[Range<u32>; 3]) -> VoxelData {
    match (data, plan) {
        (VoxelData::U8(values), Plan::Shift(plan)) => {
            VoxelData::U8(interpolate_shift(values, plan, ranges))
        }
        (VoxelData::U16(values), Plan::Shift(plan)) => {
            VoxelData::U16(interpolate_shift(values, plan, ranges))
        }
        (VoxelData::F32(values), Plan::Shift(plan)) => {
            VoxelData::F32(interpolate_shift(values, plan, ranges))
        }
        (VoxelData::U8(values), Plan::Field(plan)) => {
            VoxelData::U8(interpolate_field(values, plan, ranges))
        }
        (VoxelData::U16(values), Plan::Field(plan)) => {
            VoxelData::U16(interpolate_field(values, plan, ranges))
        }
        (VoxelData::F32(values), Plan::Field(plan)) => {
            VoxelData::F32(interpolate_field(values, plan, ranges))
        }
    }
}

pub(crate) fn zeros(format: VoxelFormat, ranges: &[Range<u32>; 3]) -> VoxelData {
    let count = [0, 1, 2]
        .iter()
        .map(|axis| (ranges[*axis].end - ranges[*axis].start) as usize)
        .product();
    match format {
        VoxelFormat::U8 => VoxelData::U8(vec![0; count]),
        VoxelFormat::U16 => VoxelData::U16(vec![0; count]),
        VoxelFormat::F32 => VoxelData::F32(vec![0.0; count]),
    }
}

fn interpolate_shift<T: Value>(data: &[T], plan: &ShiftPlan, ranges: &[Range<u32>; 3]) -> Vec<T> {
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
                            value += data[base + at].decode() * x_axis.lower_weight;
                        }
                        if let Some(at) = x_axis.upper[x] {
                            value += data[base + at].decode() * x_axis.upper_weight;
                        }
                        *slot += value * weight;
                    }
                }
            }
            values.extend(row.iter().map(|value| T::encode(*value)));
        }
    }
    values
}

fn interpolate_field<T: Value>(data: &[T], plan: &FieldPlan, ranges: &[Range<u32>; 3]) -> Vec<T> {
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
    let stride = [1usize, dims[0], dims[0] * dims[1]];
    let mut values = Vec::with_capacity(width * height * depth);
    for z in 0..depth {
        for y in 0..height {
            let index = start + Vec3::new(0.0, y as f32, z as f32);
            let row = anchor + step[2] * z as f32 + step[1] * y as f32;
            for x in 0..width {
                let point = row + step[0] * x as f32;
                let at = index + Vec3::new(x as f32, 0.0, 0.0);
                let offset = inverse * Vec3::from(plan.warp.sample(point.to_array()));
                let position = at + offset;
                values.push(T::encode(sample(
                    data,
                    &dims,
                    &stride,
                    position,
                    first,
                    plan.limits,
                )));
            }
        }
    }
    values
}

fn sample<T: Value>(
    data: &[T],
    dims: &[usize; 3],
    stride: &[usize; 3],
    position: Vec3,
    first: Vec3,
    limits: [u32; 3],
) -> f32 {
    let low = [
        position.x.floor() as i64,
        position.y.floor() as i64,
        position.z.floor() as i64,
    ];
    let weight = [
        position.x - low[0] as f32,
        position.y - low[1] as f32,
        position.z - low[2] as f32,
    ];
    let mut sum = 0.0f32;
    for corner in 0..8usize {
        let mut factor = 1.0f32;
        for (axis, component) in weight.iter().enumerate() {
            factor *= if (corner >> axis) & 1 == 1 {
                *component
            } else {
                1.0 - *component
            };
        }
        if factor == 0.0 {
            continue;
        }
        let at = [
            low[0] + (corner & 1) as i64,
            low[1] + ((corner >> 1) & 1) as i64,
            low[2] + ((corner >> 2) & 1) as i64,
        ];
        if (0..3).any(|axis| at[axis] < 0 || at[axis] >= i64::from(limits[axis])) {
            continue;
        }
        let local = [
            (at[0] as f32 - first.x) as usize,
            (at[1] as f32 - first.y) as usize,
            (at[2] as f32 - first.z) as usize,
        ];
        if (0..3).any(|axis| local[axis] >= dims[axis]) {
            continue;
        }
        sum += data[local[0] * stride[0] + local[1] * stride[1] + local[2] * stride[2]].decode()
            * factor;
    }
    sum
}

trait Value: Copy {
    fn decode(self) -> f32;
    fn encode(value: f32) -> Self;
}

impl Value for u8 {
    fn decode(self) -> f32 {
        f32::from(self)
    }

    fn encode(value: f32) -> Self {
        (value + 0.5).clamp(0.0, f32::from(u8::MAX)) as Self
    }
}

impl Value for u16 {
    fn decode(self) -> f32 {
        f32::from(self)
    }

    fn encode(value: f32) -> Self {
        (value + 0.5).clamp(0.0, f32::from(u16::MAX)) as Self
    }
}

impl Value for f32 {
    fn decode(self) -> f32 {
        self
    }

    fn encode(value: f32) -> Self {
        value
    }
}
