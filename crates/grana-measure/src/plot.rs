use crate::correlation::Pearson;
use crate::table::Table;

pub const HISTOGRAM_BINS: usize = 48;
pub const SCATTER_BINS: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureView {
    Table,
    Histogram,
    Scatter,
}

impl MeasureView {
    pub const ALL: [Self; 3] = [Self::Table, Self::Histogram, Self::Scatter];

    pub fn index(self) -> u8 {
        match self {
            Self::Table => 0,
            Self::Histogram => 1,
            Self::Scatter => 2,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Histogram => "histogram",
            Self::Scatter => "scatter",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.name() == key)
    }

    pub fn plot(self) -> Option<Self> {
        match self {
            Self::Table => None,
            Self::Histogram | Self::Scatter => Some(self),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotAxis {
    X,
    Y,
}

impl PlotAxis {
    pub const ALL: [Self; 2] = [Self::X, Self::Y];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    count: u64,
    mean: f64,
    deviation: f64,
    minimum: f64,
    maximum: f64,
}

impl Summary {
    pub fn count(self) -> u64 {
        self.count
    }

    pub fn mean(self) -> f64 {
        self.mean
    }

    pub fn deviation(self) -> f64 {
        self.deviation
    }

    pub fn minimum(self) -> f64 {
        self.minimum
    }

    pub fn maximum(self) -> f64 {
        self.maximum
    }
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Scatter {
    x: usize,
    y: usize,
    x_low: f32,
    x_high: f32,
    y_low: f32,
    y_high: f32,
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

    pub fn total_at(&self, column: usize, row: usize) -> u64 {
        self.total[row * SCATTER_BINS + column]
    }

    pub fn gated_at(&self, column: usize, row: usize) -> u64 {
        self.gated[row * SCATTER_BINS + column]
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

#[derive(Clone, Debug, PartialEq)]
pub enum Plot {
    Histogram(Histogram),
    Scatter(Scatter),
}

impl Plot {
    pub fn view(&self) -> MeasureView {
        match self {
            Self::Histogram(_) => MeasureView::Histogram,
            Self::Scatter(_) => MeasureView::Scatter,
        }
    }
}

#[derive(Clone, Copy)]
struct Moments {
    count: u64,
    sum: f64,
    squares: f64,
    minimum: f64,
    maximum: f64,
}

impl Moments {
    fn new() -> Self {
        Self {
            count: 0,
            sum: 0.0,
            squares: 0.0,
            minimum: f64::INFINITY,
            maximum: f64::NEG_INFINITY,
        }
    }

    fn push(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        let value = f64::from(value);
        self.count += 1;
        self.sum += value;
        self.squares += value * value;
        self.minimum = self.minimum.min(value);
        self.maximum = self.maximum.max(value);
    }

    fn summary(self) -> Summary {
        if self.count == 0 {
            return Summary::default();
        }
        let count = self.count as f64;
        let mean = self.sum / count;
        let variance = (self.squares / count - mean * mean).max(0.0);
        Summary {
            count: self.count,
            mean,
            deviation: variance.sqrt(),
            minimum: self.minimum,
            maximum: self.maximum,
        }
    }
}

impl Table {
    pub fn plot(&self, view: MeasureView, axes: [usize; 2], gated: &[usize]) -> Option<Plot> {
        let columns = self.columns().len();
        assert!(
            axes.iter().all(|axis| *axis < columns),
            "a plot names columns of its table"
        );
        match view {
            MeasureView::Table => None,
            MeasureView::Histogram => Some(Plot::Histogram(self.histogram(axes[0], gated))),
            MeasureView::Scatter => Some(Plot::Scatter(self.scatter(axes[0], axes[1], gated))),
        }
    }

    fn histogram(&self, column: usize, gated: &[usize]) -> Histogram {
        let (low, high) = extent(self, column);
        let mut total = vec![0u64; HISTOGRAM_BINS];
        for row in 0..self.rows() {
            if let Some(bin) = bin(self.value(row, column), low, high, HISTOGRAM_BINS) {
                total[bin] += 1;
            }
        }
        let mut counts = vec![0u64; HISTOGRAM_BINS];
        let mut moments = Moments::new();
        for row in gated {
            let value = self.value(*row, column);
            if let Some(bin) = bin(value, low, high, HISTOGRAM_BINS) {
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

    fn scatter(&self, x: usize, y: usize, gated: &[usize]) -> Scatter {
        let x_range = extent(self, x);
        let y_range = extent(self, y);
        let (x_low, x_high) = x_range;
        let (y_low, y_high) = y_range;
        let mut total = vec![0u64; SCATTER_BINS * SCATTER_BINS];
        for row in 0..self.rows() {
            if let Some(index) = cell(self, row, x, y, x_range, y_range) {
                total[index] += 1;
            }
        }
        let mut counts = vec![0u64; SCATTER_BINS * SCATTER_BINS];
        let mut x_moments = Moments::new();
        let mut y_moments = Moments::new();
        let mut correlation = Pearson::default();
        for row in gated {
            let first = self.value(*row, x);
            let second = self.value(*row, y);
            if let Some(index) = cell(self, *row, x, y, x_range, y_range) {
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
            x_low,
            x_high,
            y_low,
            y_high,
            total,
            gated: counts,
            peak,
            x_summary: x_moments.summary(),
            y_summary: y_moments.summary(),
            correlation: correlation.value(),
        }
    }
}

fn cell(
    table: &Table,
    row: usize,
    x: usize,
    y: usize,
    x_range: (f32, f32),
    y_range: (f32, f32),
) -> Option<usize> {
    let column = bin(table.value(row, x), x_range.0, x_range.1, SCATTER_BINS)?;
    let line = bin(table.value(row, y), y_range.0, y_range.1, SCATTER_BINS)?;
    Some(line * SCATTER_BINS + column)
}

fn extent(table: &Table, column: usize) -> (f32, f32) {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for row in 0..table.rows() {
        let value = table.value(row, column);
        if value.is_finite() {
            low = low.min(value);
            high = high.max(value);
        }
    }
    if !(low.is_finite() && high.is_finite()) {
        return (0.0, 1.0);
    }
    if high > low {
        return (low, high);
    }
    let padding = (0.5 * low.abs()).max(0.5);
    (low - padding, high + padding)
}

fn bin(value: f32, low: f32, high: f32, bins: usize) -> Option<usize> {
    if !value.is_finite() {
        return None;
    }
    let span = high - low;
    if span <= 0.0 {
        return Some(0);
    }
    let position = ((value - low) / span * bins as f32).floor();
    Some((position as isize).clamp(0, bins as isize - 1) as usize)
}
