use crate::sample::Sample;

const MINIMUM_CELL: f32 = 1.0e-5;

const RADII: usize = 24;

const REACH: f32 = 0.25;

const SAMPLE_LIMIT: usize = 20_000;

const SIMULATIONS: u32 = 39;

const SIMULATION_MINIMUM: u32 = 9;

const SIMULATION_VISITS: u64 = 200_000_000;

const VISIT_NEIGHBOURS: u64 = 27;

const SIMULATION_SEED: u64 = 0x5153_5441_5449_5354;

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
    let dims = [0, 1, 2].map(|axis| ((size[axis] / cell).ceil() as usize).max(1));
    let width = [0, 1, 2].map(|axis| size[axis] / dims[axis] as f32);
    let cell_of = |point: [f32; 3]| {
        [0, 1, 2].map(|axis| {
            (((point[axis] - low[axis]) / width[axis]).floor() as usize).min(dims[axis] - 1) as i32
        })
    };
    let slot = |coord: [i32; 3]| {
        (coord[2] as usize * dims[1] + coord[1] as usize) * dims[0] + coord[0] as usize
    };
    let mut grid = vec![Vec::<u32>::new(); dims[0] * dims[1] * dims[2]];
    for (index, point) in points.iter().enumerate() {
        grid[slot(cell_of(*point))].push(index as u32);
    }
    let reach = 2i32;
    let mut nearest = vec![f32::INFINITY; count];
    for (index, point) in points.iter().enumerate() {
        let origin = cell_of(*point);
        let bounds = [0, 1, 2].map(|axis| {
            [
                (origin[axis] - reach).max(0),
                (origin[axis] + reach).min(dims[axis] as i32 - 1),
            ]
        });
        for z in bounds[2][0]..=bounds[2][1] {
            for y in bounds[1][0]..=bounds[1][1] {
                for x in bounds[0][0]..=bounds[0][1] {
                    for other in &grid[slot([x, y, z])] {
                        if *other as usize == index {
                            continue;
                        }
                        let offset = points[*other as usize];
                        let distance = (0..3)
                            .map(|axis| {
                                let delta = offset[axis] - point[axis];
                                delta * delta
                            })
                            .sum::<f32>();
                        nearest[index] = nearest[index].min(distance);
                    }
                }
            }
        }
        let reachable = outside_squared(bounds, low, width, dims, *point);
        if nearest[index] > reachable {
            nearest[index] = f32::INFINITY;
            for (other, offset) in points.iter().enumerate() {
                if other == index {
                    continue;
                }
                let distance = (0..3)
                    .map(|axis| {
                        let delta = offset[axis] - point[axis];
                        delta * delta
                    })
                    .sum::<f32>();
                nearest[index] = nearest[index].min(distance);
            }
        }
        nearest[index] = nearest[index].sqrt();
    }
    nearest
}

fn outside_squared(
    bounds: [[i32; 2]; 3],
    low: [f32; 3],
    width: [f32; 3],
    dims: [usize; 3],
    point: [f32; 3],
) -> f32 {
    let mut clearance = f32::INFINITY;
    for axis in 0..3 {
        if bounds[axis][0] == 0 && bounds[axis][1] as usize == dims[axis] - 1 {
            continue;
        }
        let start = low[axis] + bounds[axis][0] as f32 * width[axis];
        let end = low[axis] + (bounds[axis][1] + 1) as f32 * width[axis];
        clearance = clearance.min((point[axis] - start).min(end - point[axis]));
    }
    let clearance = clearance.max(0.0);
    clearance * clearance
}

#[derive(Clone, Debug, PartialEq)]
pub struct RipleyPoint {
    radius: f32,
    frames: u64,
    value: f32,
    lower: f32,
    upper: f32,
    envelope_low: Option<f32>,
    envelope_high: Option<f32>,
}

impl RipleyPoint {
    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn lower(&self) -> f32 {
        self.lower
    }

    pub fn upper(&self) -> f32 {
        self.upper
    }

    pub fn envelope(&self) -> Option<(f32, f32)> {
        self.envelope_low.zip(self.envelope_high)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ripley {
    points: Vec<RipleyPoint>,
    low: f32,
    high: f32,
    radius: f32,
    frames: u32,
    objects: u64,
    simulations: u32,
}

impl Ripley {
    pub fn points(&self) -> &[RipleyPoint] {
        &self.points
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn objects(&self) -> u64 {
        self.objects
    }

    pub fn simulations(&self) -> u32 {
        self.simulations
    }
}

pub(crate) fn ripley(frames: &[Vec<[f32; 3]>], window: Window) -> Option<Ripley> {
    let radius = REACH * window.reach();
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let step = radius / RADII as f32;
    let radii = (1..=RADII)
        .map(|order| step * order as f32)
        .collect::<Vec<_>>();
    let volume = window.volume();
    let mut samples = vec![Vec::<f64>::new(); RADII];
    let mut objects = 0u64;
    let mut estimated = 0u32;
    let mut sampled = Vec::<Vec<[f32; 3]>>::new();
    for points in frames {
        if points.len() < 2 {
            continue;
        }
        objects += points.len() as u64;
        let stride = points.len().div_ceil(SAMPLE_LIMIT).max(1);
        let held = if stride > 1 {
            points.iter().step_by(stride).copied().collect::<Vec<_>>()
        } else {
            points.clone()
        };
        if held.len() < 2 {
            continue;
        }
        estimated += 1;
        let values = frame_l(&held, window, radius, volume, step, &radii);
        for (order, value) in values.iter().enumerate() {
            if value.is_finite() {
                samples[order].push(*value);
            }
        }
        sampled.push(held);
    }
    if objects == 0 || estimated == 0 {
        return None;
    }
    let simulations = simulations(sampled.iter().map(|points| points.len() as u64).sum());
    let envelope = envelope(&sampled, window, radius, volume, step, &radii, simulations);
    let mut points = Vec::with_capacity(RADII);
    let mut low = 0.0f32;
    let mut high = 0.0f32;
    for (order, radius) in radii.iter().enumerate() {
        let Some(sample) = Sample::new(std::mem::take(&mut samples[order])) else {
            break;
        };
        let (lower, upper) = sample
            .mean_interval(0.95)
            .unwrap_or((sample.mean(), sample.mean()));
        let value = sample.mean() as f32;
        let (envelope_low, envelope_high) = envelope[order]
            .map(|(low, high)| (low as f32, high as f32))
            .unzip();
        low = low
            .min((lower as f32).min(value))
            .min(envelope_low.unwrap_or(f32::INFINITY));
        high = high
            .max((upper as f32).max(value))
            .max(envelope_high.unwrap_or(f32::NEG_INFINITY));
        points.push(RipleyPoint {
            radius: *radius,
            frames: sample.len() as u64,
            value,
            lower: lower as f32,
            upper: upper as f32,
            envelope_low,
            envelope_high,
        });
    }
    if points.is_empty() {
        return None;
    }
    if !(low.is_finite() && high.is_finite()) {
        low = -radius;
        high = radius;
    } else if high <= low {
        let padding = (0.5 * low.abs()).max(0.5);
        low -= padding;
        high += padding;
    }
    let radius = points.last().map_or(radius, |point| point.radius);
    let simulations = if points.iter().any(|point| point.envelope().is_some()) {
        simulations
    } else {
        0
    };
    Some(Ripley {
        points,
        low,
        high,
        radius,
        frames: estimated,
        objects,
        simulations,
    })
}

fn simulations(points: u64) -> u32 {
    let visits = VISIT_NEIGHBOURS.saturating_mul(points.max(1));
    let allowed = (SIMULATION_VISITS / visits).max(1) as u32;
    allowed.clamp(SIMULATION_MINIMUM, SIMULATIONS)
}

fn envelope(
    frames: &[Vec<[f32; 3]>],
    window: Window,
    radius: f32,
    volume: f64,
    step: f32,
    radii: &[f32],
    simulations: u32,
) -> Vec<Option<(f64, f64)>> {
    let mut curves = vec![Vec::<f64>::new(); radii.len()];
    let mut rng = Generator::new(SIMULATION_SEED);
    for _ in 0..simulations {
        let mut sums = vec![0.0f64; radii.len()];
        let mut counts = vec![0u32; radii.len()];
        for points in frames {
            let mut simulated = Vec::with_capacity(points.len());
            for _ in 0..points.len() {
                simulated.push([
                    window.low[0] + (window.high[0] - window.low[0]) * rng.unit(),
                    window.low[1] + (window.high[1] - window.low[1]) * rng.unit(),
                    window.low[2] + (window.high[2] - window.low[2]) * rng.unit(),
                ]);
            }
            let values = frame_l(&simulated, window, radius, volume, step, radii);
            for (order, value) in values.iter().enumerate() {
                if value.is_finite() {
                    sums[order] += *value;
                    counts[order] += 1;
                }
            }
        }
        for (order, sum) in sums.iter().enumerate() {
            if counts[order] > 0 {
                curves[order].push(sum / f64::from(counts[order]));
            }
        }
    }
    curves
        .into_iter()
        .map(|curve| {
            (curve.len() as u32 >= SIMULATION_MINIMUM).then(|| {
                curve
                    .iter()
                    .copied()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                        (low.min(value), high.max(value))
                    })
            })
        })
        .collect()
}

fn frame_l(
    points: &[[f32; 3]],
    window: Window,
    radius: f32,
    volume: f64,
    step: f32,
    radii: &[f32],
) -> Vec<f64> {
    let mut result = vec![f64::NAN; radii.len()];
    if points.len() < 2 {
        return result;
    }
    let count = points.len() as f64;
    let clearance = points
        .iter()
        .map(|point| window.clearance(*point))
        .collect::<Vec<_>>();
    let mut ordered = clearance.clone();
    ordered.sort_by(f32::total_cmp);
    let mut pairs = [0i64; RADII + 1];
    for_each_pair(points, radius, |first, second, distance| {
        for centre in [first, second] {
            let reach = clearance[centre].min(radius);
            if distance > reach {
                continue;
            }
            let start = ((distance / step).ceil() as i64 - 1).max(0);
            let end = (reach / step).floor() as i64 - 1;
            if end < start {
                continue;
            }
            let end = end.min(RADII as i64 - 1) as usize;
            pairs[start as usize] += 1;
            pairs[end + 1] -= 1;
        }
    });
    let mut counted = 0i64;
    for (order, radius) in radii.iter().enumerate() {
        counted += pairs[order];
        let centres = ordered.len() - ordered.partition_point(|value| *value < *radius);
        if centres == 0 {
            break;
        }
        let expected = volume * counted as f64 / (count * centres as f64);
        result[order] = (3.0 * expected / (4.0 * std::f64::consts::PI))
            .max(0.0)
            .cbrt()
            - f64::from(*radius);
    }
    result
}

struct Generator {
    state: u64,
}

impl Generator {
    fn new(seed: u64) -> Self {
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

fn for_each_pair(points: &[[f32; 3]], radius: f32, mut visit: impl FnMut(usize, usize, f32)) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let cell = radius.max(MINIMUM_CELL);
    let dims = [0, 1, 2].map(|axis| (((high[axis] - low[axis]) / cell).ceil() as usize).max(1));
    let cell_of = |point: [f32; 3]| {
        [0, 1, 2].map(|axis| {
            (((point[axis] - low[axis]) / cell).floor() as usize).min(dims[axis] - 1) as i32
        })
    };
    let slot = |coord: [i32; 3]| {
        (coord[2] as usize * dims[1] + coord[1] as usize) * dims[0] + coord[0] as usize
    };
    let mut grid = vec![Vec::<u32>::new(); dims[0] * dims[1] * dims[2]];
    for (index, point) in points.iter().enumerate() {
        grid[slot(cell_of(*point))].push(index as u32);
    }
    let squared = radius * radius;
    for (index, point) in points.iter().enumerate() {
        let origin = cell_of(*point);
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    let coord = [origin[0] + x, origin[1] + y, origin[2] + z];
                    if (0..3).any(|axis| coord[axis] < 0 || coord[axis] as usize >= dims[axis]) {
                        continue;
                    }
                    for other in &grid[slot(coord)] {
                        if *other as usize <= index {
                            continue;
                        }
                        let offset = points[*other as usize];
                        let distance = (0..3)
                            .map(|axis| {
                                let delta = offset[axis] - point[axis];
                                delta * delta
                            })
                            .sum::<f32>();
                        if distance <= squared {
                            visit(index, *other as usize, distance.sqrt());
                        }
                    }
                }
            }
        }
    }
}
