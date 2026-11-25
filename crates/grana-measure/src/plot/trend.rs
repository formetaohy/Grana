use std::collections::BTreeMap;

use crate::mask::Mask;
use crate::sample::Sample;
use crate::table::{Group, Table};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Population {
    Object,
    Track,
    Frame,
    Molecule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendPoint {
    time: u32,
    count: u64,
    value: f32,
    deviation: Option<f32>,
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

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn deviation(&self) -> Option<f32> {
        self.deviation
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
    population: Population,
}

impl Trend {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn population(&self) -> Population {
        self.population
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
    pub fn trend(&self, column: usize, gated: &Mask) -> Trend {
        let aggregate = self.aggregate();
        let (buckets, total, frames) = self.trend_buckets(column, gated);
        let mut points = Vec::with_capacity(buckets.len());
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        let mut gated_count = 0u64;
        for (time, values) in buckets {
            let Some(sample) = Sample::new(values) else {
                continue;
            };
            let Some((value, interval)) = aggregate.of_sample(&sample) else {
                continue;
            };
            let (lower, upper) = interval.unwrap_or((value, value));
            low = low.min(lower as f32);
            high = high.max(upper as f32);
            gated_count += sample.len() as u64;
            points.push(TrendPoint {
                time,
                count: sample.len() as u64,
                value: value as f32,
                deviation: sample.deviation().map(|deviation| deviation as f32),
                lower: lower as f32,
                upper: upper as f32,
            });
        }
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
            population: self.population(),
        }
    }

    fn population(&self) -> Population {
        match self.group() {
            Group::Object => Population::Object,
            Group::Track => Population::Track,
            Group::Timepoint => Population::Frame,
            Group::Molecule => Population::Molecule,
        }
    }

    fn trend_buckets(&self, column: usize, gated: &Mask) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        match self.group() {
            Group::Object | Group::Timepoint => self.row_buckets(column, gated),
            Group::Track | Group::Molecule => self.lifetime_buckets(column, gated),
        }
    }

    fn lifetime_buckets(&self, column: usize, gated: &Mask) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        let first = self
            .column("first_frame")
            .expect("a grouped table spans the frames of its rows");
        let last = self
            .column("last_frame")
            .expect("a grouped table spans the frames of its rows");
        let values = self.materialize(column);
        let first = self.materialize(first);
        let last = self.materialize(last);
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        let mut total = 0u64;
        let mut span = 0u32;
        for row in 0..self.rows() {
            let Some(value) = values[row].present() else {
                continue;
            };
            let (Some(start), Some(end)) = (first[row].present(), last[row].present()) else {
                continue;
            };
            if !(start >= 1.0 && end >= start) {
                continue;
            }
            let start = start as u32 - 1;
            let end = end as u32 - 1;
            span = span.max(end + 1);
            total += u64::from(end - start + 1);
            if !gated.contains(row) {
                continue;
            }
            for time in start..=end {
                buckets.entry(time).or_default().push(f64::from(value));
            }
        }
        (buckets, total, span)
    }

    fn row_buckets(&self, column: usize, gated: &Mask) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        let values = self.materialize(column);
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        let mut total = 0u64;
        let mut span = 0u32;
        for row in 0..self.rows() {
            let Some(time) = self.observation(row).time() else {
                continue;
            };
            span = span.max(time + 1);
            let Some(value) = values[row].present() else {
                continue;
            };
            total += 1;
            if gated.contains(row) {
                buckets.entry(time).or_default().push(f64::from(value));
            }
        }
        (buckets, total, span)
    }
}
