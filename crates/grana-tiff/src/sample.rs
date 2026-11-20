use grana_volume::VoxelFormat;

const FORMAT_UINT: u16 = 1;
const FORMAT_INT: u16 = 2;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_VOID: u16 = 4;
const FORMAT_COMPLEX_INT: u16 = 5;
const FORMAT_COMPLEX_FLOAT: u16 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SampleType {
    pub bits: u16,
    pub format: SampleFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SampleFormat {
    Unsigned,
    Signed,
    Float,
    ComplexSigned,
    ComplexFloat,
}

impl SampleType {
    pub(crate) fn of(bits: u16, format: u16) -> Result<Self, String> {
        let format = match format {
            FORMAT_UINT | FORMAT_VOID => SampleFormat::Unsigned,
            FORMAT_INT => SampleFormat::Signed,
            FORMAT_FLOAT => SampleFormat::Float,
            FORMAT_COMPLEX_INT => SampleFormat::ComplexSigned,
            FORMAT_COMPLEX_FLOAT => SampleFormat::ComplexFloat,
            other => return Err(format!("the TIFF sample format {other} is unknown")),
        };
        if bits == 0 || bits > 64 {
            return Err(format!("the TIFF sample width of {bits} bits is not valid"));
        }
        Ok(Self { bits, format })
    }

    pub(crate) fn container(self) -> Result<VoxelFormat, String> {
        let format = match (self.format, self.bits) {
            (SampleFormat::Unsigned, 1..=8) => VoxelFormat::U8,
            (SampleFormat::Unsigned, 9..=16) => VoxelFormat::U16,
            (SampleFormat::Unsigned, 17..=32) => VoxelFormat::U32,
            (SampleFormat::Unsigned, 33..=64) => VoxelFormat::U64,
            (SampleFormat::Signed, 1..=8) | (SampleFormat::ComplexSigned, 1..=8) => VoxelFormat::I8,
            (SampleFormat::Signed, 9..=16) | (SampleFormat::ComplexSigned, 9..=16) => {
                VoxelFormat::I16
            }
            (SampleFormat::Signed, 17..=32) | (SampleFormat::ComplexSigned, 17..=32) => {
                VoxelFormat::I32
            }
            (SampleFormat::Signed, 33..=64) | (SampleFormat::ComplexSigned, 33..=64) => {
                VoxelFormat::I64
            }
            (SampleFormat::Float, 16 | 32) | (SampleFormat::ComplexFloat, 16 | 32) => {
                VoxelFormat::F32
            }
            (SampleFormat::Float, 64) | (SampleFormat::ComplexFloat, 64) => VoxelFormat::F64,
            (SampleFormat::Float, bits) | (SampleFormat::ComplexFloat, bits) => {
                return Err(format!("the TIFF stores {bits}-bit floating point samples"));
            }
            _ => return Err("the TIFF stores an unusable sample width".to_string()),
        };
        Ok(format)
    }

    pub(crate) fn components(self) -> u32 {
        match self.format {
            SampleFormat::ComplexSigned | SampleFormat::ComplexFloat => 2,
            SampleFormat::Unsigned | SampleFormat::Signed | SampleFormat::Float => 1,
        }
    }

    pub(crate) fn packed(self) -> bool {
        self.bits < 8
    }

    pub(crate) fn maximum(self) -> f32 {
        if matches!(
            self.format,
            SampleFormat::Float | SampleFormat::ComplexFloat
        ) {
            return 1.0;
        }
        if matches!(
            self.format,
            SampleFormat::Signed | SampleFormat::ComplexSigned
        ) {
            return ((1i64 << (self.bits - 1)) - 1) as f32;
        }
        if self.bits >= 64 {
            u64::MAX as f32
        } else {
            ((1u64 << self.bits) - 1) as f32
        }
    }
}
