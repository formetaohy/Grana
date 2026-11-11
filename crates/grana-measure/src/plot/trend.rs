use std::collections::{BTreeMap, BTreeSet};

use crate::mask::Mask;
use crate::sample::Sample;
use crate::table::{Group, Table};

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
            let (value, interval) = aggregate.of_sample(&sample);
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
        }
    }

    fn trend_buckets(&self, column: usize, gated: &Mask) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        if let Some(basis) = self.basis() {
            if let Some(source) = self.source(column) {
                return self.raw_buckets(basis, source, gated);
            }
            if self.group() == Group::Track
                && let Some(buckets) = self.lifetime_buckets(column, gated)
            {
                return buckets;
            }
        }
        self.row_buckets(column, gated)
    }

    fn raw_buckets(
        &self,
        basis: &Table,
        source: usize,
        gated: &Mask,
    ) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        let frames = match self.group() {
            Group::Object | Group::Track => BTreeSet::new(),
            Group::Timepoint => gated
                .iter()
                .filter_map(|row| self.observation(row).time())
                .collect::<BTreeSet<_>>(),
        };
        let tracks = match self.group() {
            Group::Object | Group::Timepoint => BTreeSet::new(),
            Group::Track => gated
                .iter()
                .filter_map(|row| self.observation(row).track())
                .collect::<BTreeSet<_>>(),
        };
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        let mut total = 0u64;
        let mut span = 0u32;
        for row in 0..basis.rows() {
            let Some(time) = basis.observation(row).time() else {
                continue;
            };
            span = span.max(time + 1);
            let value = basis.value(row, source);
            if !value.is_finite() {
                continue;
            }
            total += 1;
            let admitted = match self.group() {
                Group::Object => true,
                Group::Timepoint => frames.contains(&time),
                Group::Track => basis
                    .observation(row)
                    .track()
                    .is_some_and(|track| tracks.contains(&track)),
            };
            if admitted {
                buckets.entry(time).or_default().push(f64::from(value));
            }
        }
        (buckets, total, span)
    }

    fn lifetime_buckets(
        &self,
        column: usize,
        gated: &Mask,
    ) -> Option<(BTreeMap<u32, Vec<f64>>, u64, u32)> {
        let first = self.column("first_frame")?;
        let last = self.column("last_frame")?;
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        let mut total = 0u64;
        let mut span = 0u32;
        for row in 0..self.rows() {
            let value = self.value(row, column);
            if !value.is_finite() {
                continue;
            }
            let start = self.value(row, first);
            let end = self.value(row, last);
            if !(start.is_finite() && end.is_finite() && start >= 1.0 && end >= start) {
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
        Some((buckets, total, span))
    }

    fn row_buckets(&self, column: usize, gated: &Mask) -> (BTreeMap<u32, Vec<f64>>, u64, u32) {
        let mut buckets = BTreeMap::<u32, Vec<f64>>::new();
        let mut total = 0u64;
        let mut span = 0u32;
        for row in 0..self.rows() {
            let Some(time) = self.observation(row).time() else {
                continue;
            };
            span = span.max(time + 1);
            let value = self.value(row, column);
            if !value.is_finite() {
                continue;
            }
            total += 1;
            if gated.contains(row) {
                buckets.entry(time).or_default().push(f64::from(value));
            }
        }
        (buckets, total, span)
    }
}
