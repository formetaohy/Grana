use crate::{VoxelData, VoxelFormat, VoxelValue};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoData {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl NoData {
    pub fn zero(format: VoxelFormat) -> Self {
        match format {
            VoxelFormat::U8 => Self::U8(0),
            VoxelFormat::U16 => Self::U16(0),
            VoxelFormat::U32 => Self::U32(0),
            VoxelFormat::U64 => Self::U64(0),
            VoxelFormat::I8 => Self::I8(0),
            VoxelFormat::I16 => Self::I16(0),
            VoxelFormat::I32 => Self::I32(0),
            VoxelFormat::I64 => Self::I64(0),
            VoxelFormat::F32 => Self::F32(0.0),
            VoxelFormat::F64 => Self::F64(0.0),
        }
    }

    pub fn of(format: VoxelFormat, value: f64) -> Self {
        match format {
            VoxelFormat::U8 => Self::U8(<u8 as VoxelValue>::from_f64(value)),
            VoxelFormat::U16 => Self::U16(<u16 as VoxelValue>::from_f64(value)),
            VoxelFormat::U32 => Self::U32(<u32 as VoxelValue>::from_f64(value)),
            VoxelFormat::U64 => Self::U64(<u64 as VoxelValue>::from_f64(value)),
            VoxelFormat::I8 => Self::I8(<i8 as VoxelValue>::from_f64(value)),
            VoxelFormat::I16 => Self::I16(<i16 as VoxelValue>::from_f64(value)),
            VoxelFormat::I32 => Self::I32(<i32 as VoxelValue>::from_f64(value)),
            VoxelFormat::I64 => Self::I64(<i64 as VoxelValue>::from_f64(value)),
            VoxelFormat::F32 => Self::F32(<f32 as VoxelValue>::from_f64(value)),
            VoxelFormat::F64 => Self::F64(value),
        }
    }

    pub fn declared(self) -> Option<Self> {
        (self != Self::zero(self.format())).then_some(self)
    }

    pub fn matches_sample(self, value: f32) -> bool {
        let held = self.value() as f32;
        held == value || (held.is_nan() && value.is_nan())
    }

    pub fn format(self) -> VoxelFormat {
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

    pub fn value(self) -> f64 {
        match self {
            Self::U8(value) => VoxelValue::to_f64(value),
            Self::U16(value) => VoxelValue::to_f64(value),
            Self::U32(value) => VoxelValue::to_f64(value),
            Self::U64(value) => VoxelValue::to_f64(value),
            Self::I8(value) => VoxelValue::to_f64(value),
            Self::I16(value) => VoxelValue::to_f64(value),
            Self::I32(value) => VoxelValue::to_f64(value),
            Self::I64(value) => VoxelValue::to_f64(value),
            Self::F32(value) => VoxelValue::to_f64(value),
            Self::F64(value) => value,
        }
    }

    pub fn filled(self, count: usize) -> VoxelData {
        match self {
            Self::U8(value) => VoxelData::U8(vec![value; count]),
            Self::U16(value) => VoxelData::U16(vec![value; count]),
            Self::U32(value) => VoxelData::U32(vec![value; count]),
            Self::U64(value) => VoxelData::U64(vec![value; count]),
            Self::I8(value) => VoxelData::I8(vec![value; count]),
            Self::I16(value) => VoxelData::I16(vec![value; count]),
            Self::I32(value) => VoxelData::I32(vec![value; count]),
            Self::I64(value) => VoxelData::I64(vec![value; count]),
            Self::F32(value) => VoxelData::F32(vec![value; count]),
            Self::F64(value) => VoxelData::F64(vec![value; count]),
        }
    }

    pub fn holds(self, value: &VoxelData, index: usize) -> bool {
        match (self, value) {
            (Self::U8(held), VoxelData::U8(data)) => held == data[index],
            (Self::U16(held), VoxelData::U16(data)) => held == data[index],
            (Self::U32(held), VoxelData::U32(data)) => held == data[index],
            (Self::U64(held), VoxelData::U64(data)) => held == data[index],
            (Self::I8(held), VoxelData::I8(data)) => held == data[index],
            (Self::I16(held), VoxelData::I16(data)) => held == data[index],
            (Self::I32(held), VoxelData::I32(data)) => held == data[index],
            (Self::I64(held), VoxelData::I64(data)) => held == data[index],
            (Self::F32(held), VoxelData::F32(data)) => {
                held == data[index] || (held.is_nan() && data[index].is_nan())
            }
            (Self::F64(held), VoxelData::F64(data)) => {
                held == data[index] || (held.is_nan() && data[index].is_nan())
            }
            _ => panic!("the no-data value holds the format of its volume"),
        }
    }
}
