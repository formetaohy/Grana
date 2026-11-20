use std::ops::Range;

use crate::{Extent, Intensity};

pub const UNIT_MAXIMUM: f32 = 65535.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoxelFormat {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoxelData {
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
}

#[derive(Clone, Copy, Debug)]
pub struct VoxelUnits<'a> {
    data: &'a VoxelData,
    minimum: f32,
    scale: f32,
}

pub(crate) trait Sample: Copy + 'static {
    fn to_f32(self) -> f32;
    fn to_f64(self) -> f64;
}

impl VoxelFormat {
    pub fn bytes(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    pub fn display_maximum(self) -> f32 {
        match self {
            Self::U8 => f32::from(u8::MAX),
            Self::U16 => f32::from(u16::MAX),
            Self::U32 => u32::MAX as f32,
            Self::U64 => u64::MAX as f32,
            Self::I8 => f32::from(i8::MAX),
            Self::I16 => f32::from(i16::MAX),
            Self::I32 => i32::MAX as f32,
            Self::I64 => i64::MAX as f32,
            Self::F32 | Self::F64 => 1.0,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::U8 => 0,
            Self::U16 => 1,
            Self::F32 => 2,
            Self::U32 => 3,
            Self::I8 => 4,
            Self::I16 => 5,
            Self::I32 => 6,
            Self::F64 => 7,
            Self::U64 => 8,
            Self::I64 => 9,
        }
    }

    pub fn union(self, other: Self) -> Option<Self> {
        use VoxelFormat::{F32, F64, I8, I16, I32, I64, U8, U16, U32, U64};
        match (self, other) {
            (held, other) if held == other => Some(held),
            (U8, U16) | (U16, U8) => Some(U16),
            (U8 | U16, U32) | (U32, U8 | U16) => Some(U32),
            (U8 | U16 | U32, U64) | (U64, U8 | U16 | U32) => Some(U64),
            (I8, I16) | (I16, I8) => Some(I16),
            (I8 | I16, I32) | (I32, I8 | I16) => Some(I32),
            (I8 | I16 | I32, I64) | (I64, I8 | I16 | I32) => Some(I64),
            (U8, I8 | I16) | (I8 | I16, U8) => Some(I16),
            (U16, I8 | I16) | (I8 | I16, U16) => Some(I32),
            (U8 | U16, I32) | (I32, U8 | U16) => Some(I32),
            (U32, I8 | I16 | I32) | (I8 | I16 | I32, U32) => Some(I64),
            (U8 | U16 | U32, I64) | (I64, U8 | U16 | U32) => Some(I64),
            (U8 | U16, F32) | (F32, U8 | U16) => Some(F32),
            (I8 | I16, F32) | (F32, I8 | I16) => Some(F32),
            (U32 | I32, F32) | (F32, U32 | I32) => Some(F64),
            (U8 | U16 | U32, F64) | (F64, U8 | U16 | U32) => Some(F64),
            (I8 | I16 | I32, F64) | (F64, I8 | I16 | I32) => Some(F64),
            (F32, F64) | (F64, F32) => Some(F64),
            _ => None,
        }
    }
}

impl VoxelData {
    pub fn format(&self) -> VoxelFormat {
        match self {
            Self::U8(_) => VoxelFormat::U8,
            Self::U16(_) => VoxelFormat::U16,
            Self::U32(_) => VoxelFormat::U32,
            Self::U64(_) => VoxelFormat::U64,
            Self::I8(_) => VoxelFormat::I8,
            Self::I16(_) => VoxelFormat::I16,
            Self::I32(_) => VoxelFormat::I32,
            Self::I64(_) => VoxelFormat::I64,
            Self::F32(_) => VoxelFormat::F32,
            Self::F64(_) => VoxelFormat::F64,
        }
    }

    pub fn voxel_count(&self) -> usize {
        match self {
            Self::U8(data) => data.len(),
            Self::U16(data) => data.len(),
            Self::U32(data) => data.len(),
            Self::U64(data) => data.len(),
            Self::I8(data) => data.len(),
            Self::I16(data) => data.len(),
            Self::I32(data) => data.len(),
            Self::I64(data) => data.len(),
            Self::F32(data) => data.len(),
            Self::F64(data) => data.len(),
        }
    }

    pub fn bytes(&self) -> u64 {
        (self.voxel_count() * self.format().bytes()) as u64
    }

    pub fn blank(format: VoxelFormat, count: usize) -> Self {
        match format {
            VoxelFormat::U8 => Self::U8(vec![0; count]),
            VoxelFormat::U16 => Self::U16(vec![0; count]),
            VoxelFormat::U32 => Self::U32(vec![0; count]),
            VoxelFormat::U64 => Self::U64(vec![0; count]),
            VoxelFormat::I8 => Self::I8(vec![0; count]),
            VoxelFormat::I16 => Self::I16(vec![0; count]),
            VoxelFormat::I32 => Self::I32(vec![0; count]),
            VoxelFormat::I64 => Self::I64(vec![0; count]),
            VoxelFormat::F32 => Self::F32(vec![0.0; count]),
            VoxelFormat::F64 => Self::F64(vec![0.0; count]),
        }
    }

    pub fn reinterpret(format: VoxelFormat, bytes: &[u8]) -> Self {
        assert!(
            bytes.len().is_multiple_of(format.bytes()),
            "the bytes hold whole samples of the format"
        );
        macro_rules! decode {
            ($variant:ident, $type:ty) => {
                Self::$variant(
                    bytes
                        .chunks_exact(size_of::<$type>())
                        .map(|sample| {
                            <$type>::from_ne_bytes(
                                sample.try_into().expect("a whole sample is read"),
                            )
                        })
                        .collect(),
                )
            };
        }
        match format {
            VoxelFormat::U8 => Self::U8(bytes.to_vec()),
            VoxelFormat::U16 => decode!(U16, u16),
            VoxelFormat::U32 => decode!(U32, u32),
            VoxelFormat::U64 => decode!(U64, u64),
            VoxelFormat::I8 => Self::I8(bytes.iter().map(|byte| *byte as i8).collect()),
            VoxelFormat::I16 => decode!(I16, i16),
            VoxelFormat::I32 => decode!(I32, i32),
            VoxelFormat::I64 => decode!(I64, i64),
            VoxelFormat::F32 => decode!(F32, f32),
            VoxelFormat::F64 => decode!(F64, f64),
        }
    }

    pub fn from_values(format: VoxelFormat, count: usize, mut next: impl FnMut() -> f64) -> Self {
        macro_rules! build {
            ($variant:ident, $type:ty, $minimum:expr, $maximum:expr) => {
                Self::$variant(
                    (0..count)
                        .map(|_| next().round().clamp($minimum, $maximum) as $type)
                        .collect(),
                )
            };
        }
        match format {
            VoxelFormat::U8 => build!(U8, u8, f64::from(u8::MIN), f64::from(u8::MAX)),
            VoxelFormat::U16 => build!(U16, u16, f64::from(u16::MIN), f64::from(u16::MAX)),
            VoxelFormat::U32 => build!(U32, u32, f64::from(u32::MIN), f64::from(u32::MAX)),
            VoxelFormat::U64 => build!(U64, u64, 0.0, u64::MAX as f64),
            VoxelFormat::I8 => build!(I8, i8, f64::from(i8::MIN), f64::from(i8::MAX)),
            VoxelFormat::I16 => build!(I16, i16, f64::from(i16::MIN), f64::from(i16::MAX)),
            VoxelFormat::I32 => build!(I32, i32, f64::from(i32::MIN), f64::from(i32::MAX)),
            VoxelFormat::I64 => build!(I64, i64, i64::MIN as f64, i64::MAX as f64),
            VoxelFormat::F32 => Self::F32((0..count).map(|_| next() as f32).collect()),
            VoxelFormat::F64 => Self::F64((0..count).map(|_| next()).collect()),
        }
    }

    pub fn cast(self, format: VoxelFormat) -> Self {
        if self.format() == format {
            return self;
        }
        assert_eq!(
            self.format().union(format),
            Some(format),
            "a voxel cast widens the sample type"
        );
        macro_rules! cast {
            ($data:expr) => {
                match format {
                    VoxelFormat::U8 => Self::U8($data.iter().map(|value| *value as u8).collect()),
                    VoxelFormat::U16 => {
                        Self::U16($data.iter().map(|value| *value as u16).collect())
                    }
                    VoxelFormat::U32 => {
                        Self::U32($data.iter().map(|value| *value as u32).collect())
                    }
                    VoxelFormat::U64 => {
                        Self::U64($data.iter().map(|value| *value as u64).collect())
                    }
                    VoxelFormat::I8 => Self::I8($data.iter().map(|value| *value as i8).collect()),
                    VoxelFormat::I16 => {
                        Self::I16($data.iter().map(|value| *value as i16).collect())
                    }
                    VoxelFormat::I32 => {
                        Self::I32($data.iter().map(|value| *value as i32).collect())
                    }
                    VoxelFormat::I64 => {
                        Self::I64($data.iter().map(|value| *value as i64).collect())
                    }
                    VoxelFormat::F32 => {
                        Self::F32($data.iter().map(|value| *value as f32).collect())
                    }
                    VoxelFormat::F64 => {
                        Self::F64($data.iter().map(|value| *value as f64).collect())
                    }
                }
            };
        }
        match &self {
            Self::U8(data) => cast!(data),
            Self::U16(data) => cast!(data),
            Self::U32(data) => cast!(data),
            Self::U64(data) => cast!(data),
            Self::I8(data) => cast!(data),
            Self::I16(data) => cast!(data),
            Self::I32(data) => cast!(data),
            Self::I64(data) => cast!(data),
            Self::F32(data) => cast!(data),
            Self::F64(data) => cast!(data),
        }
    }

    pub fn invert(&mut self) {
        match self {
            Self::U8(data) => data.iter_mut().for_each(|value| *value = !*value),
            Self::U16(data) => data.iter_mut().for_each(|value| *value = !*value),
            Self::U32(data) => data.iter_mut().for_each(|value| *value = !*value),
            Self::U64(data) => data.iter_mut().for_each(|value| *value = !*value),
            Self::I8(data) => data
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            Self::I16(data) => data
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            Self::I32(data) => data
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            Self::I64(data) => data
                .iter_mut()
                .for_each(|value| *value = value.wrapping_neg()),
            Self::F32(data) => data.iter_mut().for_each(|value| *value = 1.0 - *value),
            Self::F64(data) => data.iter_mut().for_each(|value| *value = 1.0 - *value),
        }
    }

    pub fn blit(&mut self, source: &Self, from: Placement, to: Placement, columns: u32, rows: u32) {
        assert_eq!(
            self.format(),
            source.format(),
            "a blit copies the same voxel format"
        );
        let from = from.usize();
        let to = to.usize();
        let columns = columns as usize;
        let rows = rows as usize;
        macro_rules! blit {
            ($variant:ident) => {
                if let (Self::$variant(target), Self::$variant(source)) = (&mut *self, source) {
                    blit_rows(target, to, source, from, columns, rows);
                }
            };
        }
        blit!(U8);
        blit!(U16);
        blit!(U32);
        blit!(U64);
        blit!(I8);
        blit!(I16);
        blit!(I32);
        blit!(I64);
        blit!(F32);
        blit!(F64);
    }

    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            Self::U8(data) => data.clone(),
            Self::I8(data) => data.iter().map(|value| *value as u8).collect(),
            Self::U16(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::U32(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::U64(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::I16(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::I32(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::I64(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::F32(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
            Self::F64(data) => data.iter().flat_map(|value| value.to_le_bytes()).collect(),
        }
    }

    pub fn from_le_bytes(format: VoxelFormat, bytes: &[u8]) -> Self {
        assert!(
            bytes.len().is_multiple_of(format.bytes()),
            "the bytes hold whole samples of the format"
        );
        macro_rules! decode {
            ($variant:ident, $type:ty) => {
                Self::$variant(
                    bytes
                        .chunks_exact(size_of::<$type>())
                        .map(|sample| {
                            <$type>::from_le_bytes(
                                sample.try_into().expect("a whole sample is read"),
                            )
                        })
                        .collect(),
                )
            };
        }
        match format {
            VoxelFormat::U8 => Self::U8(bytes.to_vec()),
            VoxelFormat::U16 => decode!(U16, u16),
            VoxelFormat::U32 => decode!(U32, u32),
            VoxelFormat::U64 => decode!(U64, u64),
            VoxelFormat::I8 => Self::I8(bytes.iter().map(|byte| *byte as i8).collect()),
            VoxelFormat::I16 => decode!(I16, i16),
            VoxelFormat::I32 => decode!(I32, i32),
            VoxelFormat::I64 => decode!(I64, i64),
            VoxelFormat::F32 => decode!(F32, f32),
            VoxelFormat::F64 => decode!(F64, f64),
        }
    }

    pub fn gather(&self, offset: usize, stride: usize, count: usize) -> Self {
        match self {
            Self::U8(data) => Self::U8(gather(data, offset, stride, count)),
            Self::U16(data) => Self::U16(gather(data, offset, stride, count)),
            Self::U32(data) => Self::U32(gather(data, offset, stride, count)),
            Self::U64(data) => Self::U64(gather(data, offset, stride, count)),
            Self::I8(data) => Self::I8(gather(data, offset, stride, count)),
            Self::I16(data) => Self::I16(gather(data, offset, stride, count)),
            Self::I32(data) => Self::I32(gather(data, offset, stride, count)),
            Self::I64(data) => Self::I64(gather(data, offset, stride, count)),
            Self::F32(data) => Self::F32(gather(data, offset, stride, count)),
            Self::F64(data) => Self::F64(gather(data, offset, stride, count)),
        }
    }

    pub fn range(&self) -> Intensity {
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        self.for_each_value(|value| {
            let value = f64::from(value);
            if value.is_finite() {
                if value < minimum {
                    minimum = value;
                }
                if value > maximum {
                    maximum = value;
                }
            }
        });
        if minimum > maximum {
            return Intensity::new(0.0, 0.0);
        }
        Intensity::new(
            minimum.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
            maximum.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
        )
    }

    pub fn units(&self, intensity: Intensity) -> VoxelUnits<'_> {
        let span = intensity.span();
        VoxelUnits {
            data: self,
            minimum: intensity.min(),
            scale: if span > 0.0 { UNIT_MAXIMUM / span } else { 0.0 },
        }
    }

    pub fn write_values(&self, out: &mut [f32]) {
        assert_eq!(
            out.len(),
            self.voxel_count(),
            "the destination must hold every voxel"
        );
        let mut index = 0;
        self.for_each_value(|value| {
            out[index] = value;
            index += 1;
        });
    }

    pub fn value(&self, index: usize) -> f32 {
        match self {
            Self::U8(data) => value_of(data, index),
            Self::U16(data) => value_of(data, index),
            Self::U32(data) => value_of(data, index),
            Self::U64(data) => value_of(data, index),
            Self::I8(data) => value_of(data, index),
            Self::I16(data) => value_of(data, index),
            Self::I32(data) => value_of(data, index),
            Self::I64(data) => value_of(data, index),
            Self::F32(data) => value_of(data, index),
            Self::F64(data) => value_of(data, index),
        }
    }

    pub fn wide_value(&self, index: usize) -> f64 {
        match self {
            Self::U8(data) => wide_of(data, index),
            Self::U16(data) => wide_of(data, index),
            Self::U32(data) => wide_of(data, index),
            Self::U64(data) => wide_of(data, index),
            Self::I8(data) => wide_of(data, index),
            Self::I16(data) => wide_of(data, index),
            Self::I32(data) => wide_of(data, index),
            Self::I64(data) => wide_of(data, index),
            Self::F32(data) => wide_of(data, index),
            Self::F64(data) => wide_of(data, index),
        }
    }

    pub fn region(&self, extent: Extent, x: Range<u32>, y: Range<u32>, z: Range<u32>) -> Self {
        assert!(
            x.start < x.end && x.end <= extent.x(),
            "the x range lies inside the extent"
        );
        assert!(
            y.start < y.end && y.end <= extent.y(),
            "the y range lies inside the extent"
        );
        assert!(
            z.start < z.end && z.end <= extent.z(),
            "the z range lies inside the extent"
        );
        match self {
            Self::U8(data) => Self::U8(extract(data, extent, &x, &y, &z)),
            Self::U16(data) => Self::U16(extract(data, extent, &x, &y, &z)),
            Self::U32(data) => Self::U32(extract(data, extent, &x, &y, &z)),
            Self::U64(data) => Self::U64(extract(data, extent, &x, &y, &z)),
            Self::I8(data) => Self::I8(extract(data, extent, &x, &y, &z)),
            Self::I16(data) => Self::I16(extract(data, extent, &x, &y, &z)),
            Self::I32(data) => Self::I32(extract(data, extent, &x, &y, &z)),
            Self::I64(data) => Self::I64(extract(data, extent, &x, &y, &z)),
            Self::F32(data) => Self::F32(extract(data, extent, &x, &y, &z)),
            Self::F64(data) => Self::F64(extract(data, extent, &x, &y, &z)),
        }
    }

    pub fn for_each_value(&self, visit: impl FnMut(f32)) {
        match self {
            Self::U8(data) => for_each(data, visit),
            Self::U16(data) => for_each(data, visit),
            Self::U32(data) => for_each(data, visit),
            Self::U64(data) => for_each(data, visit),
            Self::I8(data) => for_each(data, visit),
            Self::I16(data) => for_each(data, visit),
            Self::I32(data) => for_each(data, visit),
            Self::I64(data) => for_each(data, visit),
            Self::F32(data) => for_each(data, visit),
            Self::F64(data) => for_each(data, visit),
        }
    }
}

impl VoxelUnits<'_> {
    pub fn visit(&self, range: Range<usize>, visit: impl FnMut(f32)) {
        assert!(
            range.start < range.end && range.end <= self.data.voxel_count(),
            "a unit visit covers voxels inside the data"
        );
        let minimum = self.minimum;
        let scale = self.scale;
        let unit = move |value: f32| {
            if scale > 0.0 && value.is_finite() {
                ((value - minimum) * scale).clamp(0.0, UNIT_MAXIMUM)
            } else {
                0.0
            }
        };
        match self.data {
            VoxelData::U8(data) => visit_range(data, range, unit, visit),
            VoxelData::U16(data) => visit_range(data, range, unit, visit),
            VoxelData::U32(data) => visit_range(data, range, unit, visit),
            VoxelData::U64(data) => visit_range(data, range, unit, visit),
            VoxelData::I8(data) => visit_range(data, range, unit, visit),
            VoxelData::I16(data) => visit_range(data, range, unit, visit),
            VoxelData::I32(data) => visit_range(data, range, unit, visit),
            VoxelData::I64(data) => visit_range(data, range, unit, visit),
            VoxelData::F32(data) => visit_range(data, range, unit, visit),
            VoxelData::F64(data) => visit_range(data, range, unit, visit),
        }
    }
}

fn for_each<T: Sample>(data: &[T], mut visit: impl FnMut(f32)) {
    data.iter().for_each(|value| visit(value.to_f32()));
}

fn value_of<T: Sample>(data: &[T], index: usize) -> f32 {
    assert!(index < data.len(), "the voxel lies inside the volume");
    data[index].to_f32()
}

fn wide_of<T: Sample>(data: &[T], index: usize) -> f64 {
    assert!(index < data.len(), "the voxel lies inside the volume");
    data[index].to_f64()
}

fn gather<T: Copy>(data: &[T], offset: usize, stride: usize, count: usize) -> Vec<T> {
    assert!(
        stride > 0,
        "a gathered sample advances by at least one slot"
    );
    assert!(
        offset + stride * count.saturating_sub(1) < data.len(),
        "a gathered sample lies inside the data"
    );
    (0..count)
        .map(|index| data[offset + index * stride])
        .collect()
}

pub struct Placement {
    width: u32,
    origin: [u32; 2],
}

impl Placement {
    pub fn new(width: u32, origin: [u32; 2]) -> Self {
        Self { width, origin }
    }

    fn usize(self) -> (usize, [usize; 2]) {
        (self.width as usize, self.origin.map(|value| value as usize))
    }
}

fn blit_rows<T: Copy>(
    target: &mut [T],
    to: (usize, [usize; 2]),
    source: &[T],
    from: (usize, [usize; 2]),
    columns: usize,
    rows: usize,
) {
    let (target_width, target_origin) = to;
    let (source_width, source_origin) = from;
    assert!(
        source.len() >= source_width * (source_origin[1] + rows),
        "a blitted block holds its source rectangle"
    );
    for row in 0..rows {
        let start = (target_origin[1] + row) * target_width + target_origin[0];
        let at = (source_origin[1] + row) * source_width + source_origin[0];
        target[start..start + columns].copy_from_slice(&source[at..at + columns]);
    }
}

fn visit_range<T: Sample>(
    data: &[T],
    range: Range<usize>,
    unit: impl Fn(f32) -> f32,
    mut visit: impl FnMut(f32),
) {
    data[range]
        .iter()
        .for_each(|value| visit(unit(value.to_f32())));
}

fn extract<T: Copy>(
    source: &[T],
    extent: Extent,
    x: &Range<u32>,
    y: &Range<u32>,
    z: &Range<u32>,
) -> Vec<T> {
    let width = (x.end - x.start) as usize;
    let mut values =
        Vec::with_capacity(width * (y.end - y.start) as usize * (z.end - z.start) as usize);
    for z in z.clone() {
        for y in y.clone() {
            let start = extent.index(x.start, y, z) as usize;
            values.extend_from_slice(&source[start..start + width]);
        }
    }
    values
}

macro_rules! sample {
    ($type:ty) => {
        impl Sample for $type {
            fn to_f32(self) -> f32 {
                self as f32
            }

            fn to_f64(self) -> f64 {
                self as f64
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
    fn to_f32(self) -> f32 {
        self
    }

    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl Sample for f64 {
    fn to_f32(self) -> f32 {
        self as f32
    }

    fn to_f64(self) -> f64 {
        self
    }
}
