use std::ops::Range;

use grana_object::Mask;
use grana_volume::{Intensity, Pyramid, Region, VoxelData};

use crate::channels::Channels;

const SAMPLE_PLANES: u32 = 8;
const SAMPLE_VOXELS: usize = 1 << 18;
const PERCENTILE: usize = 100;
const COARSE_SIDE: u32 = 96;

pub(super) struct Leveling {
    intensity: Intensity,
    low: f32,
    high: f32,
}

impl Leveling {
    pub(super) fn at(
        reader: &Channels,
        time: u32,
        channel: usize,
        level: usize,
        ranges: &[Range<u32>; 3],
        mask: Option<&Mask>,
        shift: [f32; 3],
    ) -> Result<Self, String> {
        Self::collect(reader, time, channel, level, ranges, mask, shift)
    }

    pub(super) fn sampled(
        reader: &Channels,
        time: u32,
        channel: usize,
        region: Region,
    ) -> Result<Self, String> {
        let pyramid = reader.pyramid();
        let level = coarse_level(pyramid);
        let info = pyramid.level(level);
        let ranges = region.voxel_range(info).unwrap_or_else(|| {
            let extent = info.extent();
            [0..extent.x(), 0..extent.y(), 0..extent.z()]
        });
        Self::collect(reader, time, channel, level, &ranges, None, [0.0; 3])
    }

    fn collect(
        reader: &Channels,
        time: u32,
        channel: usize,
        level: usize,
        ranges: &[Range<u32>; 3],
        mask: Option<&Mask>,
        shift: [f32; 3],
    ) -> Result<Self, String> {
        let intensity = reader.pyramid().channels()[channel].intensity();
        let width = (ranges[0].end - ranges[0].start) as usize;
        let plane = width * (ranges[1].end - ranges[1].start) as usize;
        let depth = ranges[2].end - ranges[2].start;
        let planes = SAMPLE_PLANES.min(depth).max(1);
        let stride = (depth / planes).max(1);
        let step = (plane * planes as usize / SAMPLE_VOXELS).max(1);
        let mut samples = Vec::with_capacity(SAMPLE_VOXELS + plane);
        let mut z = ranges[2].start;
        while z < ranges[2].end {
            let slice = [ranges[0].clone(), ranges[1].clone(), z..z + 1];
            let values = reader
                .read_region(time, level, channel, &slice, shift)
                .map_err(|error| error.to_string())?;
            let mut voxels = vec![0.0f32; values.voxel_count()];
            values.write_values(&mut voxels);
            for (index, value) in voxels.iter().enumerate().step_by(step) {
                if let Some(mask) = mask {
                    let x = ranges[0].start + (index % width) as u32;
                    let y = ranges[1].start + (index / width) as u32;
                    if !mask.contains_cell([x, y, z]) {
                        continue;
                    }
                }
                samples.push(intensity.unit(*value));
            }
            z += stride;
        }
        Ok(Self::from_samples(intensity, samples))
    }

    fn from_samples(intensity: Intensity, mut samples: Vec<f32>) -> Self {
        if samples.is_empty() {
            return Self {
                intensity,
                low: 0.0,
                high: 0.0,
            };
        }
        samples.sort_unstable_by(f32::total_cmp);
        let last = samples.len() - 1;
        Self {
            intensity,
            low: samples[last / PERCENTILE],
            high: samples[last - last / PERCENTILE],
        }
    }

    pub(super) fn apply(&self, values: &VoxelData) -> Vec<f32> {
        let mut plane = vec![0.0f32; values.voxel_count()];
        values.write_values(&mut plane);
        let span = self.high - self.low;
        if !span.is_finite() || span <= 0.0 {
            plane.fill(0.0);
            return plane;
        }
        for value in &mut plane {
            let unit = self.intensity.unit(*value);
            *value = ((unit - self.low) / span).clamp(0.0, 1.0);
        }
        plane
    }
}

fn coarse_level(pyramid: &Pyramid) -> usize {
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        if pyramid.level(index).extent().max_dimension() >= COARSE_SIDE {
            level = index;
        }
    }
    level
}
