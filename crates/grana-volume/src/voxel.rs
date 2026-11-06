use std::ops::Range;

use crate::{Extent, Intensity};

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

    pub fn write_units_strided(
        &self,
        intensity: Intensity,
        out: &mut [u16],
        stride: usize,
        offset: usize,
    ) {
        assert!(stride > 0, "a unit stride visits at least one slot");
        assert!(offset < stride, "a unit offset lies inside the stride");
        assert_eq!(
            out.len(),
            self.voxel_count() * stride,
            "the destination must hold every voxel at the stride"
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
            out[offset + index * stride] = unit.round() as u16;
            index += 1;
        });
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
            Self::U8(data) => {
                assert!(index < data.len(), "the voxel lies inside the volume");
                f32::from(data[index])
            }
            Self::U16(data) => {
                assert!(index < data.len(), "the voxel lies inside the volume");
                f32::from(data[index])
            }
            Self::F32(data) => {
                assert!(index < data.len(), "the voxel lies inside the volume");
                data[index]
            }
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
            Self::F32(data) => Self::F32(extract(data, extent, &x, &y, &z)),
        }
    }

    fn for_each_value(&self, mut visit: impl FnMut(f32)) {
        match self {
            Self::U8(data) => data.iter().for_each(|&value| visit(f32::from(value))),
            Self::U16(data) => data.iter().for_each(|&value| visit(f32::from(value))),
            Self::F32(data) => data.iter().for_each(|&value| visit(value)),
        }
    }
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
