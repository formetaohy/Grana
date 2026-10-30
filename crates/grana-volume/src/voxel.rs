use crate::Intensity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoxelFormat {
    U8,
    U16,
    F32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoxelData {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

impl VoxelData {
    pub fn format(&self) -> VoxelFormat {
        match self {
            Self::U8(_) => VoxelFormat::U8,
            Self::U16(_) => VoxelFormat::U16,
            Self::F32(_) => VoxelFormat::F32,
        }
    }

    pub fn voxel_count(&self) -> usize {
        match self {
            Self::U8(data) => data.len(),
            Self::U16(data) => data.len(),
            Self::F32(data) => data.len(),
        }
    }

    pub fn range(&self) -> Intensity {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        self.for_each_value(|value| {
            if value.is_finite() {
                if value < min {
                    min = value;
                }
                if value > max {
                    max = value;
                }
            }
        });
        if min > max {
            return Intensity::new(0.0, 0.0);
        }
        Intensity::new(min, max)
    }

    pub fn unit_max(&self, intensity: Intensity) -> f32 {
        let mut max = 0.0f32;
        self.for_each_value(|value| {
            let unit = intensity.unit(value);
            if unit > max {
                max = unit;
            }
        });
        max
    }

    pub fn write_units(&self, intensity: Intensity, out: &mut [u16]) {
        assert_eq!(
            out.len(),
            self.voxel_count(),
            "the destination must hold every voxel"
        );
        let span = intensity.span();
        let scale = if span > 0.0 {
            f32::from(u16::MAX) / span
        } else {
            0.0
        };
        let mut index = 0;
        self.for_each_value(|value| {
            let unit = if span > 0.0 && value.is_finite() {
                ((value - intensity.min()) * scale).clamp(0.0, f32::from(u16::MAX))
            } else {
                0.0
            };
            out[index] = unit.round() as u16;
            index += 1;
        });
    }

    fn for_each_value(&self, mut visit: impl FnMut(f32)) {
        match self {
            Self::U8(data) => data.iter().for_each(|&value| visit(f32::from(value))),
            Self::U16(data) => data.iter().for_each(|&value| visit(f32::from(value))),
            Self::F32(data) => data.iter().for_each(|&value| visit(value)),
        }
    }
}
