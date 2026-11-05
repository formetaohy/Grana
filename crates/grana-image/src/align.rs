use std::ops::Range;

use grana_volume::{VoxelData, VoxelFormat};

pub(crate) struct Plan {
    pub(crate) source: [Range<u32>; 3],
    axes: [Axis; 3],
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

pub(crate) fn level_shift(shift: [f32; 3], level: usize) -> [f32; 3] {
    assert!(
        shift.iter().all(|value| value.is_finite()),
        "an alignment offset is finite"
    );
    let ratio = (1u64 << level) as f32;
    shift.map(|value| value / ratio)
}

pub(crate) fn plan(ranges: &[Range<u32>; 3], shift: [f32; 3], limits: [u32; 3]) -> Plan {
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
    Plan { source, axes }
}

pub(crate) fn resample(data: &VoxelData, plan: &Plan, ranges: &[Range<u32>; 3]) -> VoxelData {
    match data {
        VoxelData::U8(values) => VoxelData::U8(interpolate(values, plan, ranges)),
        VoxelData::U16(values) => VoxelData::U16(interpolate(values, plan, ranges)),
        VoxelData::F32(values) => VoxelData::F32(interpolate(values, plan, ranges)),
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

fn interpolate<T: Value>(data: &[T], plan: &Plan, ranges: &[Range<u32>; 3]) -> Vec<T> {
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
