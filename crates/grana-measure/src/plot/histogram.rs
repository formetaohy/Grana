use super::bins;
use super::extent;
use super::summary::{Moments, Summary};
use crate::mask::Mask;
use crate::table::Table;

#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    column: usize,
    low: f32,
    high: f32,
    total: Vec<u64>,
    gated: Vec<u64>,
    peak: u64,
    summary: Summary,
}

impl Histogram {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn bins(&self) -> usize {
        self.total.len()
    }

    pub fn total(&self, bin: usize) -> u64 {
        self.total[bin]
    }

    pub fn gated(&self, bin: usize) -> u64 {
        self.gated[bin]
    }

    pub fn peak(&self) -> u64 {
        self.peak
    }

    pub fn summary(&self) -> Summary {
        self.summary
    }

    pub fn bin_range(&self, bin: usize) -> (f32, f32) {
        let width = (self.high - self.low) / self.bins() as f32;
        (
            self.low + width * bin as f32,
            self.low + width * (bin + 1) as f32,
        )
    }
}

impl Table {
    pub(crate) fn histogram(&self, column: usize, gated: &Mask) -> Histogram {
        let (low, high) = extent(self, column);
        let bins = bins::histogram(self, column, high - low);
        let values = self.materialize(column);
        let mut total = vec![0u64; bins];
        for row in 0..self.rows() {
            if let Some(bin) = bins::index(values[row], low, high, bins) {
                total[bin] += 1;
            }
        }
        let mut counts = vec![0u64; bins];
        let mut moments = Moments::new();
        for row in gated.iter() {
            let value = values[row];
            if let Some(bin) = bins::index(value, low, high, bins) {
                counts[bin] += 1;
            }
            moments.push(value);
        }
        let peak = total.iter().copied().max().unwrap_or(0);
        Histogram {
            column,
            low,
            high,
            total,
            gated: counts,
            peak,
            summary: moments.summary(),
        }
    }
}
