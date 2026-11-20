use std::collections::BTreeSet;
use std::ops::Range;

use crate::{Extent, Intensity, Statistics, VoxelData};

const VOXEL_BUDGET: u64 = 4 << 20;

const BLOCK_SIDE: u32 = 32;

const FRAME_LIMIT: u32 = 4;

const DISTINCT_LIMIT: usize = 1024;

pub struct Census {
    frames: u32,
    samples: u32,
    blocks: Vec<[Range<u32>; 3]>,
}

impl Census {
    pub fn of(extent: Extent, frames: u32) -> Self {
        assert!(frames > 0, "a census samples a pyramid with frames");
        let samples = frames.clamp(1, FRAME_LIMIT);
        let budget = (VOXEL_BUDGET / u64::from(samples)).max(1);
        Self {
            frames,
            samples,
            blocks: spread(extent, budget),
        }
    }

    pub fn blocks(&self) -> &[[Range<u32>; 3]] {
        &self.blocks
    }

    pub fn measure<E>(
        &mut self,
        mut read: impl FnMut(u32, [Range<u32>; 3]) -> Result<VoxelData, E>,
    ) -> Result<Statistics, E> {
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        let mut distinct = BTreeSet::new();
        for sample in 0..self.samples {
            let time = sample * self.frames / self.samples;
            for block in &self.blocks {
                let data = read(time, block.clone())?;
                data.for_each_value(|value| {
                    let value = f64::from(value);
                    if !value.is_finite() {
                        return;
                    }
                    minimum = minimum.min(value);
                    maximum = maximum.max(value);
                    if distinct.len() <= DISTINCT_LIMIT {
                        distinct.insert(value.to_bits());
                    }
                });
            }
        }
        let intensity = if minimum > maximum {
            Intensity::new(0.0, 0.0)
        } else {
            Intensity::new(minimum as f32, maximum as f32)
        };
        let counted = u32::try_from(distinct.len()).unwrap_or(u32::MAX).max(1);
        Ok(Statistics::of(intensity, counted))
    }
}

fn spread(extent: Extent, budget: u64) -> Vec<[Range<u32>; 3]> {
    let block = u64::from(BLOCK_SIDE).pow(3);
    let per_axis = (budget / block).max(1) as f64;
    let per_axis = per_axis.powf(1.0 / 3.0) as u32;
    let per_axis = per_axis.max(1);
    let steps = per_axis - 1;
    let mut blocks = Vec::with_capacity((per_axis as usize).pow(3));
    for z in 0..per_axis {
        for y in 0..per_axis {
            for x in 0..per_axis {
                let origin = [x, y, z];
                blocks.push([
                    span(origin[0], steps, BLOCK_SIDE, extent.x()),
                    span(origin[1], steps, BLOCK_SIDE, extent.y()),
                    span(origin[2], steps, BLOCK_SIDE, extent.z()),
                ]);
            }
        }
    }
    blocks
}

fn span(index: u32, steps: u32, side: u32, limit: u32) -> Range<u32> {
    let origin = if steps == 0 {
        0
    } else {
        u64::from(index) * u64::from(limit.saturating_sub(1)) / u64::from(steps)
    } as u32;
    let start = origin.min(limit.saturating_sub(1));
    let end = (start + side).min(limit);
    start..end
}
