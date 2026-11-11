use crate::sample::Sample;

const MINIMUM_CELL: f32 = 1.0e-5;

const RADII: usize = 24;

const REACH: f32 = 0.25;

const SAMPLE_LIMIT: usize = 20_000;

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ripley {
    points: Vec<RipleyPoint>,
    low: f32,
    high: f32,
    radius: f32,
    frames: u32,
    objects: u64,
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
    for points in frames {
        if points.len() < 2 {
            continue;
        }
        objects += points.len() as u64;
        let stride = points.len().div_ceil(SAMPLE_LIMIT).max(1);
        let sampled =
            (stride > 1).then(|| points.iter().step_by(stride).copied().collect::<Vec<_>>());
        let points = sampled.as_deref().unwrap_or(points);
        if points.len() < 2 {
            continue;
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
        estimated += 1;
        let mut counted = 0i64;
        for (order, radius) in radii.iter().enumerate() {
            counted += pairs[order];
            let centres = ordered.len() - ordered.partition_point(|value| *value < *radius);
            if centres == 0 {
                break;
            }
            let expected = volume * counted as f64 / (count * centres as f64);
            let l = (3.0 * expected / (4.0 * std::f64::consts::PI))
                .max(0.0)
                .cbrt()
                - f64::from(*radius);
            samples[order].push(l);
        }
    }
    if objects == 0 {
        return None;
    }
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
        low = low.min((lower as f32).min(value));
        high = high.max((upper as f32).max(value));
        points.push(RipleyPoint {
            radius: *radius,
            frames: sample.len() as u64,
            value,
            lower: lower as f32,
            upper: upper as f32,
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
    Some(Ripley {
        points,
        low,
        high,
        radius,
        frames: estimated,
        objects,
    })
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
