use crate::correlation::Pearson;
use crate::sample::Sample;
use crate::table::Table;

use std::collections::BTreeMap;

pub const HISTOGRAM_BINS: usize = 48;
pub const SCATTER_BINS: usize = 96;
pub const DENSITY_POINTS: usize = 64;

const DENSITY_SAMPLES: usize = 4096;
const OUTLIER_LIMIT: usize = 96;
const WHISKER: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureView {
    Table,
    Histogram,
    Scatter,
    Distribution,
}

impl MeasureView {
    pub const ALL: [Self; 4] = [
        Self::Table,
        Self::Histogram,
        Self::Scatter,
        Self::Distribution,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Table => 0,
            Self::Histogram => 1,
            Self::Scatter => 2,
            Self::Distribution => 3,
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
            Self::Distribution => "distribution",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.name() == key)
    }

    pub fn plot(self) -> Option<Self> {
        match self {
            Self::Table => None,
            Self::Histogram | Self::Scatter | Self::Distribution => Some(self),
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
    median: f64,
    lower: f64,
    upper: f64,
    mad: f64,
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

    pub fn median(self) -> f64 {
        self.median
    }

    pub fn lower_quartile(self) -> f64 {
        self.lower
    }

    pub fn upper_quartile(self) -> f64 {
        self.upper
    }

    pub fn mad(self) -> f64 {
        self.mad
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
pub struct DistributionSeries {
    count: u64,
    minimum: f64,
    maximum: f64,
    lower: f64,
    median: f64,
    upper: f64,
    mean: f64,
    deviation: f64,
    whisker_low: f64,
    whisker_high: f64,
    outliers: Vec<f32>,
    density: Vec<f32>,
}

impl DistributionSeries {
    pub fn of(values: Vec<f64>, range: (f32, f32)) -> Option<Self> {
        assert!(
            range.0.is_finite() && range.1.is_finite() && range.0 <= range.1,
            "a distribution series spans a finite range"
        );
        assert!(
            values.iter().all(|value| !value.is_finite()
                || (*value as f32) >= range.0 && (*value as f32) <= range.1),
            "a distribution series holds values inside its density range"
        );
        let mut count = 0u64;
        series_with(values, range.0, range.1, &mut count)
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn minimum(&self) -> f64 {
        self.minimum
    }

    pub fn maximum(&self) -> f64 {
        self.maximum
    }

    pub fn lower_quartile(&self) -> f64 {
        self.lower
    }

    pub fn median(&self) -> f64 {
        self.median
    }

    pub fn upper_quartile(&self) -> f64 {
        self.upper
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    pub fn deviation(&self) -> f64 {
        self.deviation
    }

    pub fn whisker_low(&self) -> f64 {
        self.whisker_low
    }

    pub fn whisker_high(&self) -> f64 {
        self.whisker_high
    }

    pub fn outliers(&self) -> &[f32] {
        &self.outliers
    }

    pub fn density(&self) -> &[f32] {
        &self.density
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistributionGroup {
    time: Option<u32>,
    label: String,
    total: DistributionSeries,
    gated: Option<DistributionSeries>,
}

impl DistributionGroup {
    pub fn time(&self) -> Option<u32> {
        self.time
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn total(&self) -> &DistributionSeries {
        &self.total
    }

    pub fn gated(&self) -> Option<&DistributionSeries> {
        self.gated.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Distribution {
    column: usize,
    groups: Vec<DistributionGroup>,
    low: f32,
    high: f32,
    total: u64,
    gated: u64,
}

impl Distribution {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn groups(&self) -> &[DistributionGroup] {
        &self.groups
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plot {
    Histogram(Histogram),
    Scatter(Scatter),
    Distribution(Distribution),
}

impl Plot {
    pub fn view(&self) -> MeasureView {
        match self {
            Self::Histogram(_) => MeasureView::Histogram,
            Self::Scatter(_) => MeasureView::Scatter,
            Self::Distribution(_) => MeasureView::Distribution,
        }
    }
}

#[derive(Clone)]
struct Moments {
    values: Vec<f32>,
}

impl Moments {
    fn new() -> Self {
        Self { values: Vec::new() }
    }

    fn push(&mut self, value: f32) {
        if value.is_finite() {
            self.values.push(value);
        }
    }

    fn summary(self) -> Summary {
        if self.values.is_empty() {
            return Summary::default();
        }
        let sample = Sample::new(self.values.iter().map(|value| f64::from(*value)).collect())
            .expect("a moment holds a finite value");
        Summary {
            count: sample.len() as u64,
            mean: sample.mean(),
            deviation: sample.deviation().unwrap_or(0.0),
            minimum: sample.minimum(),
            maximum: sample.maximum(),
            median: sample.median(),
            lower: sample.quantile(25.0),
            upper: sample.quantile(75.0),
            mad: sample.mad(),
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
            MeasureView::Distribution => {
                Some(Plot::Distribution(self.distribution(axes[0], gated)))
            }
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

    fn distribution(&self, column: usize, gated: &[usize]) -> Distribution {
        let (low, high) = extent(self, column);
        let timed = (0..self.rows()).all(|row| self.observation(row).time().is_some());
        let distinct = timed && {
            let mut times = (0..self.rows())
                .filter_map(|row| self.observation(row).time())
                .collect::<Vec<_>>();
            times.sort_unstable();
            times.dedup();
            times.len() == self.rows()
        };
        let split = timed && !distinct;
        let label = distribution_label(self);
        let mut totals = BTreeMap::<u32, Vec<f64>>::new();
        for row in 0..self.rows() {
            let value = self.value(row, column);
            if value.is_finite() {
                totals
                    .entry(distribution_key(self, row, split))
                    .or_default()
                    .push(f64::from(value));
            }
        }
        let gating = gated.len() != self.rows();
        let mut gated_values = BTreeMap::<u32, Vec<f64>>::new();
        if gating {
            for row in gated {
                let value = self.value(*row, column);
                if value.is_finite() {
                    gated_values
                        .entry(distribution_key(self, *row, split))
                        .or_default()
                        .push(f64::from(value));
                }
            }
        }
        let mut total = 0u64;
        let mut gated_count = 0u64;
        let mut groups = Vec::with_capacity(totals.len());
        for (key, values) in totals {
            total += values.len() as u64;
            let series = series(values, low, high);
            let gated = gated_values
                .remove(&key)
                .and_then(|values| series_with(values, low, high, &mut gated_count));
            groups.push(DistributionGroup {
                time: split.then_some(key),
                label: if split {
                    format!("Frame {}", key + 1)
                } else {
                    label.clone()
                },
                total: series,
                gated,
            });
        }
        Distribution {
            column,
            groups,
            low,
            high,
            total,
            gated: if gating { gated_count } else { total },
        }
    }
}

fn distribution_key(table: &Table, row: usize, split: bool) -> u32 {
    if split {
        table
            .observation(row)
            .time()
            .expect("a split distribution groups observations by frame")
    } else {
        0
    }
}

fn distribution_label(table: &Table) -> String {
    if (0..table.rows()).any(|row| table.observation(row).object().is_some()) {
        "All objects".to_string()
    } else if (0..table.rows()).any(|row| table.observation(row).track().is_some()) {
        "All tracks".to_string()
    } else {
        "All frames".to_string()
    }
}

fn series(values: Vec<f64>, low: f32, high: f32) -> DistributionSeries {
    let mut count = 0u64;
    series_with(values, low, high, &mut count).expect("a distribution bucket holds a finite value")
}

fn series_with(
    values: Vec<f64>,
    low: f32,
    high: f32,
    count: &mut u64,
) -> Option<DistributionSeries> {
    let sample = Sample::new(values)?;
    *count += sample.len() as u64;
    let lower = sample.quantile(25.0);
    let upper = sample.quantile(75.0);
    let span = upper - lower;
    let low_fence = lower - WHISKER * span;
    let high_fence = upper + WHISKER * span;
    let whisker_low = sample
        .values()
        .iter()
        .copied()
        .find(|value| *value >= low_fence)
        .unwrap_or_else(|| sample.minimum());
    let whisker_high = sample
        .values()
        .iter()
        .rev()
        .copied()
        .find(|value| *value <= high_fence)
        .unwrap_or_else(|| sample.maximum());
    let mut outliers = sample
        .values()
        .iter()
        .copied()
        .filter(|value| *value < low_fence || *value > high_fence)
        .collect::<Vec<_>>();
    if outliers.len() > OUTLIER_LIMIT {
        let step = outliers.len().div_ceil(OUTLIER_LIMIT);
        outliers = outliers.into_iter().step_by(step).collect();
    }
    Some(DistributionSeries {
        count: sample.len() as u64,
        minimum: sample.minimum(),
        maximum: sample.maximum(),
        lower,
        median: sample.median(),
        upper,
        mean: sample.mean(),
        deviation: sample.deviation().unwrap_or(0.0),
        whisker_low,
        whisker_high,
        outliers: outliers.into_iter().map(|value| value as f32).collect(),
        density: density(&sample, low, high),
    })
}

fn density(sample: &Sample, low: f32, high: f32) -> Vec<f32> {
    let mut points = vec![0.0f32; DENSITY_POINTS];
    let span = f64::from(high) - f64::from(low);
    if sample.is_empty() || span <= 0.0 {
        return points;
    }
    let step = sample.len().div_ceil(DENSITY_SAMPLES).max(1);
    let values = sample
        .values()
        .iter()
        .step_by(step)
        .copied()
        .collect::<Vec<_>>();
    let count = values.len() as f64;
    let deviation = sample.deviation().unwrap_or(0.0);
    let interquartile = sample.quantile(75.0) - sample.quantile(25.0);
    let spread = if interquartile > 0.0 {
        deviation.min(interquartile / 1.349)
    } else {
        deviation
    };
    let floor = span / DENSITY_POINTS as f64 * 0.75;
    let bandwidth = (0.9 * spread * count.powf(-0.2)).max(floor);
    let norm = 1.0 / (count * bandwidth * std::f64::consts::TAU.sqrt());
    let mut peak = 0.0f64;
    for (index, point) in points.iter_mut().enumerate() {
        let x = f64::from(low) + (index as f64 + 0.5) / DENSITY_POINTS as f64 * span;
        let sum = values
            .iter()
            .map(|value| (-0.5 * ((x - value) / bandwidth).powi(2)).exp())
            .sum::<f64>();
        let value = sum * norm;
        *point = value as f32;
        peak = peak.max(value);
    }
    if peak > 0.0 {
        for point in &mut points {
            *point = (*point as f64 / peak) as f32;
        }
    }
    points
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
