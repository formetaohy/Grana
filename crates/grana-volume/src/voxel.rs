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
            if value < min {
                min = value;
            }
            if value > max {
                max = value;
            }
        });
        Intensity::new(min, max)
    }

    pub fn for_each_unit(&self, intensity: Intensity, mut visit: impl FnMut(f32)) {
        self.for_each_value(|value| visit(intensity.unit(value)));
    }

    pub fn downsampled(&self, extent: Extent) -> Self {
        assert_eq!(
            self.voxel_count() as u64,
            extent.voxel_count(),
            "voxel count must match extent"
        );
        match self {
            Self::U8(data) => Self::U8(reduce(data, extent, |v| v as f32, |v| v.round() as u8)),
            Self::U16(data) => Self::U16(reduce(data, extent, |v| v as f32, |v| v.round() as u16)),
            Self::F32(data) => Self::F32(reduce(data, extent, |v| v, |v| v)),
        }
    }

    fn for_each_value(&self, mut visit: impl FnMut(f32)) {
        match self {
            Self::U8(data) => data.iter().for_each(|&value| visit(value as f32)),
            Self::U16(data) => data.iter().for_each(|&value| visit(value as f32)),
            Self::F32(data) => data.iter().for_each(|&value| visit(value)),
        }
    }
}

fn reduce<T: Copy, U>(
    source: &[T],
    extent: Extent,
    to_f32: impl Fn(T) -> f32,
    from_f32: impl Fn(f32) -> U,
) -> Vec<U> {
    let target = extent.halve();
    let mut reduced = Vec::with_capacity(target.voxel_count() as usize);
    for z in 0..target.z() {
        for y in 0..target.y() {
            for x in 0..target.x() {
                let mut sum = 0.0;
                let mut taps = 0.0;
                for dz in 0..2 {
                    let sz = (2 * z + dz).min(extent.z() - 1);
                    for dy in 0..2 {
                        let sy = (2 * y + dy).min(extent.y() - 1);
                        for dx in 0..2 {
                            let sx = (2 * x + dx).min(extent.x() - 1);
                            sum += to_f32(source[extent.index(sx, sy, sz) as usize]);
                            taps += 1.0;
                        }
                    }
                }
                reduced.push(from_f32(sum / taps));
            }
        }
    }
    reduced
}
