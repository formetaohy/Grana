use std::ops::Range;

use crate::{Extent, Intensity, VoxelData};

const VOXEL_BUDGET: u64 = 4 << 20;

const PROBE_SIDE: u32 = 32;

const PROBE_FRAMES: u32 = 4;

pub struct Probe {
    frames: u32,
    samples: u32,
    ranges: [Range<u32>; 3],
    measured: Option<Intensity>,
}

impl Probe {
    pub fn of(extent: Extent, frames: u32) -> Self {
        assert!(frames > 0, "a probe samples a pyramid with frames");
        let samples = frames.clamp(1, PROBE_FRAMES);
        let budget = (VOXEL_BUDGET / u64::from(samples)).max(1);
        let ranges = if extent.voxel_count() <= budget {
            [0..extent.x(), 0..extent.y(), 0..extent.z()]
        } else {
            [window(extent.x()), window(extent.y()), window(extent.z())]
        };
        Self {
            frames,
            samples,
            ranges,
            measured: None,
        }
    }

    pub fn measure<E>(
        &mut self,
        mut read: impl FnMut(u32, [Range<u32>; 3]) -> Result<VoxelData, E>,
    ) -> Result<Option<Intensity>, E> {
        for sample in 0..self.samples {
            let time = sample * self.frames / self.samples;
            let ranges = self.ranges.clone();
            let data = read(time, ranges)?;
            self.add(&data);
        }
        Ok(self.measured.filter(|range| range.span() > 0.0))
    }

    fn add(&mut self, data: &VoxelData) {
        let range = data.range();
        self.measured = Some(match self.measured {
            Some(held) => Intensity::new(held.min().min(range.min()), held.max().max(range.max())),
            None => range,
        });
    }
}

fn window(limit: u32) -> Range<u32> {
    let side = limit.min(PROBE_SIDE);
    let start = (limit - side) / 2;
    start..start + side
}
