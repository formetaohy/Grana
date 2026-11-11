mod grid;
mod sampling;
mod scan;
mod test;

use std::ops::Range;

use grana_volume::{Intensity, VoxelData};

pub use sampling::{ColocSampling, coloc_sampling};
pub use scan::{ColocScan, ColocScanConfig};

pub type ColocRead<'a> =
    dyn Fn(u32, usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

pub(crate) fn unit(scale: [f32; 2], value: f32) -> f32 {
    if value.is_finite() {
        ((value - scale[0]) * scale[1]).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(crate) fn scale(intensity: Intensity) -> [f32; 2] {
    let span = intensity.span();
    if span > 0.0 {
        [intensity.min(), 1.0 / span]
    } else {
        [intensity.min(), 0.0]
    }
}
