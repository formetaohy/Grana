use super::bins;
use super::extent;
use super::summary::{Moments, Summary};
use crate::correlation::Pearson;
use crate::mask::Mask;
use crate::table::Table;

#[derive(Clone, Debug, PartialEq)]
pub struct Scatter {
    x: usize,
    y: usize,
    x_low: f32,
    x_high: f32,
    y_low: f32,
    y_high: f32,
    x_bins: usize,
    y_bins: usize,
    total: Vec<u64>,
    gated: Vec<u64>,
    peak: u64,
    x_summary: Summary,
    y_summary: Summary,
    correlation: Option<f64>,
}

impl Scatter {
    pub fn x(&self) -> usize {
        self.x
    }

    pub fn y(&self) -> usize {
        self.y
    }

    pub fn x_low(&self) -> f32 {
        self.x_low
    }

    pub fn x_high(&self) -> f32 {
        self.x_high
    }

    pub fn y_low(&self) -> f32 {
        self.y_low
    }

    pub fn y_high(&self) -> f32 {
        self.y_high
    }

    pub fn x_bins(&self) -> usize {
        self.x_bins
    }

    pub fn y_bins(&self) -> usize {
        self.y_bins
    }

    pub fn total_at(&self, column: usize, row: usize) -> u64 {
        self.total[row * self.x_bins + column]
    }

    pub fn gated_at(&self, column: usize, row: usize) -> u64 {
        self.gated[row * self.x_bins + column]
    }

    pub fn peak(&self) -> u64 {
        self.peak
    }

    pub fn x_summary(&self) -> Summary {
        self.x_summary
    }

    pub fn y_summary(&self) -> Summary {
        self.y_summary
    }

    pub fn correlation(&self) -> Option<f64> {
        self.correlation
    }
}

#[derive(Clone, Copy)]
struct Grid {
    x_low: f32,
    x_high: f32,
    y_low: f32,
    y_high: f32,
    x_bins: usize,
    y_bins: usize,
}

impl Grid {
    fn of(table: &Table, x: usize, y: usize) -> Self {
        let (x_low, x_high) = extent(table, x);
        let (y_low, y_high) = extent(table, y);
        let (x_bins, y_bins) = bins::scatter(table, x, y, x_high - x_low, y_high - y_low);
        Self {
            x_low,
            x_high,
            y_low,
            y_high,
            x_bins,
            y_bins,
        }
    }

    fn cell(self, table: &Table, row: usize, x: usize, y: usize) -> Option<usize> {
        let column = bins::index(table.value(row, x), self.x_low, self.x_high, self.x_bins)?;
        let line = bins::index(table.value(row, y), self.y_low, self.y_high, self.y_bins)?;
        Some(line * self.x_bins + column)
    }
}

impl Table {
    pub(crate) fn scatter(&self, x: usize, y: usize, gated: &Mask) -> Scatter {
        let grid = Grid::of(self, x, y);
        let mut total = vec![0u64; grid.x_bins * grid.y_bins];
        for row in 0..self.rows() {
            if let Some(index) = grid.cell(self, row, x, y) {
                total[index] += 1;
            }
        }
        let mut counts = vec![0u64; grid.x_bins * grid.y_bins];
        let mut x_moments = Moments::new();
        let mut y_moments = Moments::new();
        let mut correlation = Pearson::default();
        for row in gated.iter() {
            let first = self.value(row, x);
            let second = self.value(row, y);
            if let Some(index) = grid.cell(self, row, x, y) {
                counts[index] += 1;
            }
            x_moments.push(first);
            y_moments.push(second);
            if first.is_finite() && second.is_finite() {
                correlation.push(f64::from(first), f64::from(second));
            }
        }
        let peak = total.iter().copied().max().unwrap_or(0);
        Scatter {
            x,
            y,
            x_low: grid.x_low,
            x_high: grid.x_high,
            y_low: grid.y_low,
            y_high: grid.y_high,
            x_bins: grid.x_bins,
            y_bins: grid.y_bins,
            total,
            gated: counts,
            peak,
            x_summary: x_moments.summary(),
            y_summary: y_moments.summary(),
            correlation: correlation.value(),
        }
    }
}
