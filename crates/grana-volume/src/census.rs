use std::collections::BTreeSet;
use std::ops::Range;

use rayon::prelude::*;

use crate::{Extent, Histogram, Intensity, Measured, NoData, Statistics, VoxelData};

const VOXEL_BUDGET: u64 = 4 << 20;

const BLOCK_SIDE: u32 = 32;

const FRAME_LIMIT: u32 = 4;

const DISTINCT_LIMIT: usize = 1024;

struct Band {
    z: u32,
    blocks: Vec<[Range<u32>; 3]>,
}

pub struct Census {
    frames: u32,
    samples: u32,
    bands: Vec<Band>,
}

impl Census {
    pub fn of(extent: Extent, frames: u32) -> Self {
        assert!(frames > 0, "a census samples a pyramid with frames");
        let samples = frames.clamp(1, FRAME_LIMIT);
        let budget = (VOXEL_BUDGET / u64::from(samples)).max(1);
        Self {
            frames,
            samples,
            bands: spread(extent, budget),
        }
    }

    pub fn windows(&self) -> &[[Range<u32>; 3]] {
        self.bands
            .first()
            .map_or(&[], |band| band.blocks.as_slice())
    }

    pub fn measure<E: Send>(
        &mut self,
        no_data: Option<NoData>,
        read: impl Fn(u32, [Range<u32>; 3]) -> Result<VoxelData, E> + Sync,
    ) -> Result<Measured, E> {
        let mut totals = Totals::empty();
        for sample in 0..self.samples {
            let time = sample * self.frames / self.samples;
            let bands = self
                .bands
                .par_iter()
                .map(|band| {
                    let mut held = Totals::empty();
                    for block in &band.blocks {
                        let data = read(time, block.clone())?;
                        held.add(&data, no_data);
                    }
                    Ok(held)
                })
                .collect::<Result<Vec<Totals>, E>>()?;
            for held in bands {
                totals.merge(held);
            }
        }
        let statistics = totals.statistics();
        let mut histogram = Histogram::over(statistics.intensity());
        for sample in 0..self.samples {
            let time = sample * self.frames / self.samples;
            let bands = self
                .bands
                .par_iter()
                .map(|band| {
                    let mut held = Histogram::over(statistics.intensity());
                    for block in &band.blocks {
                        let data = read(time, block.clone())?;
                        data.for_each_present_value(no_data, |value| held.add(value));
                    }
                    Ok(held)
                })
                .collect::<Result<Vec<Histogram>, E>>()?;
            for held in bands {
                histogram.merge(&held);
            }
        }
        let statistics = statistics.with_background(histogram.background());
        Ok(Measured::of(statistics, histogram))
    }
}

struct Totals {
    minimum: f64,
    maximum: f64,
    distinct: BTreeSet<u64>,
}

impl Totals {
    fn empty() -> Self {
        Self {
            minimum: f64::INFINITY,
            maximum: f64::NEG_INFINITY,
            distinct: BTreeSet::new(),
        }
    }

    fn add(&mut self, data: &VoxelData, no_data: Option<NoData>) {
        data.for_each_present_value(no_data, |value| {
            let value = f64::from(value);
            if !value.is_finite() {
                return;
            }
            self.minimum = self.minimum.min(value);
            self.maximum = self.maximum.max(value);
            if self.distinct.len() <= DISTINCT_LIMIT {
                self.distinct.insert(value.to_bits());
            }
        });
    }

    fn merge(&mut self, other: Self) {
        self.minimum = self.minimum.min(other.minimum);
        self.maximum = self.maximum.max(other.maximum);
        if self.distinct.len() <= DISTINCT_LIMIT {
            self.distinct.extend(other.distinct);
        }
    }

    fn statistics(self) -> Statistics {
        let intensity = if self.minimum > self.maximum {
            Intensity::new(0.0, 0.0)
        } else {
            Intensity::new(self.minimum as f32, self.maximum as f32)
        };
        let counted = u32::try_from(self.distinct.len())
            .unwrap_or(u32::MAX)
            .max(1);
        Statistics::of(intensity, counted)
    }
}

fn spread(extent: Extent, budget: u64) -> Vec<Band> {
    let block = u64::from(BLOCK_SIDE).pow(3);
    let per_axis = (budget / block).max(1) as f64;
    let per_axis = per_axis.powf(1.0 / 3.0) as u32;
    let per_axis = per_axis.max(1);
    let steps = per_axis - 1;
    let mut bands: Vec<Band> = Vec::new();
    for index in 0..per_axis {
        let z = span(index, steps, BLOCK_SIDE, extent.z());
        for y in 0..per_axis {
            for x in 0..per_axis {
                let block = [
                    span(x, steps, BLOCK_SIDE, extent.x()),
                    span(y, steps, BLOCK_SIDE, extent.y()),
                    0..1,
                ];
                for plane in z.clone() {
                    match bands.iter_mut().find(|band| band.z == plane) {
                        Some(band) => {
                            band.blocks
                                .push([block[0].clone(), block[1].clone(), plane..plane + 1])
                        }
                        None => bands.push(Band {
                            z: plane,
                            blocks: vec![[block[0].clone(), block[1].clone(), plane..plane + 1]],
                        }),
                    }
                }
            }
        }
    }
    bands.sort_by_key(|band| band.z);
    bands
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
