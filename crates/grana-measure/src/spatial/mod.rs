mod grid;
mod nearest;
mod ripley;

pub(crate) use nearest::nearest_neighbour;
pub use nearest::{NearestNeighbour, NearestNeighbourPoint};
pub(crate) use ripley::ripley;
pub use ripley::{Ripley, RipleyPoint, RipleyTest};

use crate::sample::Sample;
use grid::Grid;

const MINIMUM_CELL: f32 = 1.0e-5;

const RADII: usize = 24;

const REACH: f32 = 0.25;

const SAMPLE_LIMIT: usize = 20_000;

const SIMULATIONS: u32 = 199;

const SIMULATION_MINIMUM: u32 = 9;

const SIMULATION_VISITS: u64 = 400_000_000;

const VISIT_NEIGHBOURS: u64 = 27;

const SIMULATION_SEED: u64 = 0x5153_5441_5449_5354;

pub(crate) type Timed = (u32, Vec<[f32; 3]>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    low: [f32; 3],
    high: [f32; 3],
}

impl Window {
    pub fn new(low: [f32; 3], high: [f32; 3]) -> Option<Self> {
        let finite = low.iter().chain(high.iter()).all(|value| value.is_finite());
        let positive = (0..3).all(|axis| high[axis] > low[axis]);
        (finite && positive).then_some(Self { low, high })
    }

    pub fn low(self) -> [f32; 3] {
        self.low
    }

    pub fn high(self) -> [f32; 3] {
        self.high
    }

    pub fn volume(self) -> f64 {
        (0..3)
            .map(|axis| f64::from(self.high[axis] - self.low[axis]))
            .product()
    }

    pub fn contains(self, point: [f32; 3]) -> bool {
        (0..3).all(|axis| point[axis] >= self.low[axis] && point[axis] <= self.high[axis])
    }

    pub fn clearance(self, point: [f32; 3]) -> f32 {
        (0..3).fold(f32::INFINITY, |reach, axis| {
            reach
                .min(point[axis] - self.low[axis])
                .min(self.high[axis] - point[axis])
        })
    }

    pub fn reach(self) -> f32 {
        (0..3)
            .map(|axis| self.high[axis] - self.low[axis])
            .fold(f32::INFINITY, f32::min)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Census {
    frames: u32,
    valid: u32,
    single: u32,
    objects: u64,
    partners: u64,
    sampled: u64,
}

impl Census {
    pub fn frames(self) -> u32 {
        self.frames
    }

    pub fn valid(self) -> u32 {
        self.valid
    }

    pub fn single(self) -> u32 {
        self.single
    }

    pub fn objects(self) -> u64 {
        self.objects
    }

    pub fn partners(self) -> u64 {
        self.partners
    }

    pub fn sampled(self) -> u64 {
        self.sampled
    }
}

pub fn neighbour_distances(points: &[[f32; 3]]) -> Vec<f32> {
    let count = points.len();
    if count < 2 {
        return vec![f32::NAN; count];
    }
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let size = [0, 1, 2].map(|axis| (high[axis] - low[axis]).max(MINIMUM_CELL));
    let side = size.iter().copied().fold(0.0f32, f32::max);
    let cell = (side / (count as f64).cbrt() as f32).max(MINIMUM_CELL);
    let grid = Grid::new(points, cell);
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            grid.nearest(*point, f32::INFINITY, Some(index))
                .unwrap_or(f32::INFINITY)
        })
        .collect()
}

pub(crate) fn inside(points: &[[f32; 3]], window: Window) -> Vec<[f32; 3]> {
    points
        .iter()
        .copied()
        .filter(|point| window.contains(*point))
        .collect()
}

pub(crate) fn sampled(points: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let stride = points.len().div_ceil(SAMPLE_LIMIT).max(1);
    if stride > 1 {
        points.into_iter().step_by(stride).collect()
    } else {
        points
    }
}

pub(crate) fn radii(window: Window) -> Option<(Vec<f32>, f32)> {
    let radius = REACH * window.reach();
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let step = radius / RADII as f32;
    let radii = (1..=RADII)
        .map(|order| step * order as f32)
        .collect::<Vec<_>>();
    Some((radii, step))
}

pub(crate) fn uniform(rng: &mut Generator, window: Window, count: usize) -> Vec<[f32; 3]> {
    (0..count)
        .map(|_| {
            [
                window.low[0] + (window.high[0] - window.low[0]) * rng.unit(),
                window.low[1] + (window.high[1] - window.low[1]) * rng.unit(),
                window.low[2] + (window.high[2] - window.low[2]) * rng.unit(),
            ]
        })
        .collect()
}

pub(crate) fn simulations(points: u64) -> u32 {
    let visits = VISIT_NEIGHBOURS.saturating_mul(points.max(1));
    let allowed = (SIMULATION_VISITS / visits).max(1) as u32;
    allowed.clamp(SIMULATION_MINIMUM, SIMULATIONS)
}

pub(crate) struct Generator {
    state: u64,
}

impl Generator {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn unit(&mut self) -> f32 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value = value ^ (value >> 31);
        (value >> 40) as f32 / 16_777_216.0
    }
}

pub(crate) struct Envelope {
    centre: Vec<Option<f64>>,
    low: Vec<Option<f64>>,
    high: Vec<Option<f64>>,
    statistics: Vec<f64>,
}

impl Envelope {
    pub(crate) fn of(curves: Vec<Vec<Option<f64>>>, radii: usize) -> Self {
        let centre = (0..radii)
            .map(|order| {
                let values = curves
                    .iter()
                    .filter_map(|curve| curve[order])
                    .collect::<Vec<_>>();
                (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
            })
            .collect::<Vec<_>>();
        let mut statistics = Vec::with_capacity(curves.len());
        for curve in &curves {
            let mut peak = 0.0f64;
            let mut seen = false;
            for (order, value) in curve.iter().enumerate() {
                let (Some(value), Some(centre)) = (*value, centre[order]) else {
                    continue;
                };
                seen = true;
                peak = peak.max((value - centre).abs());
            }
            if seen {
                statistics.push(peak);
            }
        }
        let reach = if statistics.len() as u32 >= SIMULATION_MINIMUM {
            Sample::of(&statistics).map(|sample| sample.quantile(95.0))
        } else {
            None
        };
        let low = centre
            .iter()
            .map(|value| reach.zip(*value).map(|(reach, value)| value - reach))
            .collect();
        let high = centre
            .iter()
            .map(|value| reach.zip(*value).map(|(reach, value)| value + reach))
            .collect();
        Self {
            centre,
            low,
            high,
            statistics,
        }
    }

    pub(crate) fn band(&self, order: usize) -> (Option<f32>, Option<f32>) {
        (
            self.low[order].map(|value| value as f32),
            self.high[order].map(|value| value as f32),
        )
    }

    pub(crate) fn beyond(&self, order: usize, value: f64) -> bool {
        match (self.low[order], self.high[order]) {
            (Some(low), Some(high)) => value < low || value > high,
            _ => false,
        }
    }

    pub(crate) fn test(&self, observed: &[f64]) -> Option<(f64, u32)> {
        if self.statistics.len() < SIMULATION_MINIMUM as usize {
            return None;
        }
        let mut statistic = 0.0f64;
        let mut seen = false;
        for (order, value) in observed.iter().enumerate() {
            let Some(centre) = self.centre.get(order).copied().flatten() else {
                continue;
            };
            if !value.is_finite() {
                continue;
            }
            seen = true;
            statistic = statistic.max((value - centre).abs());
        }
        if !seen {
            return None;
        }
        let count = self.statistics.len() as u32;
        let exceedances = self
            .statistics
            .iter()
            .filter(|value| **value >= statistic)
            .count() as u32;
        Some((
            (1.0 + f64::from(exceedances)) / (1.0 + f64::from(count)),
            count,
        ))
    }
}
