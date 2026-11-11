use std::collections::BTreeMap;

use crate::mask::Mask;
use crate::sample::Sample;
use crate::table::Table;

#[derive(Clone, Debug, PartialEq)]
pub struct TrendPoint {
    time: u32,
    count: u64,
    mean: f32,
    lower: f32,
    upper: f32,
}

impl TrendPoint {
    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn mean(&self) -> f32 {
        self.mean
    }

    pub fn lower(&self) -> f32 {
        self.lower
    }

    pub fn upper(&self) -> f32 {
        self.upper
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trend {
    column: usize,
    points: Vec<TrendPoint>,
    low: f32,
    high: f32,
    total: u64,
    gated: u64,
    frames: u32,
}

impl Trend {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn points(&self) -> &[TrendPoint] {
        &self.points
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn gated(&self) -> u64 {
        self.gated
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }
}

impl Table {
    pub(crate) fn trend(&self, column: usize, gated: &Mask) -> Trend {
        let mut total = 0u64;
        for row in 0..self.rows() {
            if self.observation(row).time().is_some() && self.value(row, column).is_finite() {
                total += 1;
            }
        }
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        for row in gated.iter() {
            let Some(time) = self.observation(row).time() else {
                continue;
            };
            let value = self.value(row, column);
            if value.is_finite() {
                buckets.entry(time).or_default().push(f64::from(value));
            }
        }
        let mut points = Vec::with_capacity(buckets.len());
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        let mut gated_count = 0u64;
        for (time, values) in buckets {
            let sample = Sample::new(values).expect("a trend bucket holds a finite value");
            let (lower, upper) = sample
                .mean_interval(0.95)
                .unwrap_or((sample.mean(), sample.mean()));
            let mean = sample.mean() as f32;
            low = low.min(lower as f32);
            high = high.max(upper as f32);
            gated_count += sample.len() as u64;
            points.push(TrendPoint {
                time,
                count: sample.len() as u64,
                mean,
                lower: lower as f32,
                upper: upper as f32,
            });
        }
        let frames = points.last().map_or(0, |point| point.time + 1);
        if !(low.is_finite() && high.is_finite()) {
            low = 0.0;
            high = 1.0;
        } else if high <= low {
            let padding = (0.5 * low.abs()).max(0.5);
            low -= padding;
            high += padding;
        }
        Trend {
            column,
            points,
            low,
            high,
            total,
            gated: gated_count,
            frames,
        }
    }
}
