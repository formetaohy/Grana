use std::collections::BTreeMap;

use grana_object::{ObjectId, TrackDetection};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::marker::{Marker, TrackRow};
use crate::truth::Truth;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    frames: u32,
    seed: u64,
    cells: usize,
    capacity: usize,
    radius: f32,
    anisotropy: f32,
    extent: [f32; 3],
    step: f32,
    division: f32,
    death: f32,
    birth: f32,
    miss: f32,
    false_alarm: f32,
    spread: f32,
    growth: f32,
    swirl: f32,
    crowded: f32,
    noise: f32,
}

impl Spec {
    pub fn new(frames: u32, seed: u64) -> Self {
        assert!(frames >= 2, "a phantom series spans at least two frames");
        Self {
            frames,
            seed,
            cells: 8,
            capacity: 32,
            radius: 1.5,
            anisotropy: 2.0,
            extent: [64.0, 64.0, 32.0],
            step: 0.35,
            division: 0.06,
            death: 0.01,
            birth: 0.15,
            miss: 0.02,
            false_alarm: 0.1,
            spread: 0.15,
            growth: 0.04,
            swirl: 0.015,
            crowded: 0.5,
            noise: 0.03,
        }
    }

    pub fn uniform(self) -> Self {
        Self {
            spread: 0.0,
            growth: 0.0,
            swirl: 0.0,
            crowded: 0.0,
            noise: 0.0,
            ..self
        }
    }

    pub fn with_spread(mut self, spread: f32) -> Self {
        assert!(
            spread.is_finite() && spread >= 0.0,
            "a phantom radius spread is a non-negative log scale"
        );
        self.spread = spread;
        self
    }

    pub fn with_growth(mut self, growth: f32) -> Self {
        assert!(
            growth.is_finite() && growth >= 0.0,
            "a phantom growth rate is a non-negative fraction"
        );
        self.growth = growth;
        self
    }

    pub fn with_swirl(mut self, swirl: f32) -> Self {
        assert!(
            swirl.is_finite() && swirl.abs() < 0.5,
            "a phantom swirl is a small per-frame rotation"
        );
        self.swirl = swirl;
        self
    }

    pub fn with_crowded(mut self, crowded: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&crowded),
            "a phantom crowding is a fraction of births"
        );
        self.crowded = crowded;
        self
    }

    pub fn with_noise(mut self, noise: f32) -> Self {
        assert!(
            (0.0..1.0).contains(&noise),
            "a phantom detection noise is a fraction of the radius"
        );
        self.noise = noise;
        self
    }

    pub fn with_cells(mut self, cells: usize) -> Self {
        assert!(cells > 0, "a phantom series starts cells");
        self.cells = cells;
        self
    }

    pub fn with_extent(mut self, extent: [f32; 3]) -> Self {
        assert!(
            extent.iter().all(|value| value.is_finite() && *value > 0.0),
            "a phantom series spans a positive extent"
        );
        self.extent = extent;
        self
    }

    pub fn with_step(mut self, step: f32) -> Self {
        assert!(
            step.is_finite() && step > 0.0,
            "a phantom step is a positive length"
        );
        self.step = step;
        self
    }

    pub fn with_division(mut self, division: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&division),
            "a phantom division rate is a fraction"
        );
        self.division = division;
        self
    }

    pub fn with_miss(mut self, miss: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&miss),
            "a phantom dropout rate is a fraction"
        );
        self.miss = miss;
        self
    }

    pub fn with_false_alarm(mut self, false_alarm: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&false_alarm),
            "a phantom false alarm rate is a fraction"
        );
        self.false_alarm = false_alarm;
        self
    }

    pub fn frames(self) -> u32 {
        self.frames
    }

    pub fn extent(self) -> [f32; 3] {
        self.extent
    }
}

#[derive(Clone, Copy)]
struct Cell {
    label: u32,
    position: [f32; 3],
    velocity: [f32; 3],
    scale: f32,
    rate: f32,
    siblings: u32,
}

fn radii(spec: Spec, scale: f32) -> [f32; 3] {
    let radius = spec.radius * scale;
    [radius, radius, radius * spec.anisotropy]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Phantom {
    truth: Truth,
    detections: Vec<Vec<TrackDetection>>,
}

impl Phantom {
    pub fn of(spec: Spec) -> Self {
        let mut rng = StdRng::seed_from_u64(spec.seed);
        let mut live = (0..spec.cells)
            .map(|index| {
                let scale = scale_of(&mut rng, spec);
                let label = index as u32 + 1;
                Cell {
                    label,
                    position: spawn(&mut rng, spec.extent, radii(spec, scale)),
                    velocity: drift(&mut rng, spec.step),
                    scale,
                    rate: rate_of(&mut rng, spec),
                    siblings: 0,
                }
            })
            .collect::<Vec<_>>();
        let mut next = live.len() as u32 + 1;
        let mut parents = BTreeMap::<u32, u32>::new();
        let mut frames = Vec::with_capacity(spec.frames as usize);
        let mut detections = Vec::with_capacity(spec.frames as usize);
        for time in 0..spec.frames {
            let mut markers = Vec::with_capacity(live.len());
            let mut found = Vec::with_capacity(live.len());
            for cell in &live {
                let truth = radii(spec, cell.scale);
                markers.push(Marker::new(cell.label, time, cell.position, truth));
                found.push(measure(
                    &mut rng,
                    spec,
                    ObjectId::new(cell.label),
                    cell.position,
                    truth,
                ));
            }
            frames.push(markers);
            detections.push(found);
            if time + 1 == spec.frames {
                break;
            }
            divide(&mut rng, spec, &mut live, &mut parents, &mut next);
            retire(&mut rng, spec, &mut live);
            if rng.random::<f32>() < spec.birth && live.len() < spec.capacity {
                let scale = scale_of(&mut rng, spec);
                live.push(Cell {
                    label: next,
                    position: birthplace(&mut rng, spec, &live, radii(spec, scale)),
                    velocity: drift(&mut rng, spec.step),
                    scale,
                    rate: rate_of(&mut rng, spec),
                    siblings: 0,
                });
                next += 1;
            }
            separate(spec, &mut live);
            for cell in &mut live {
                cell.siblings = 0;
            }
            advance(&mut rng, spec, &mut live);
        }
        degrade(spec, &mut detections, &mut next);
        let mut spans = BTreeMap::<u32, (u32, u32)>::new();
        for (time, markers) in frames.iter().enumerate() {
            for marker in markers {
                let span = spans
                    .entry(marker.label())
                    .or_insert((time as u32, time as u32));
                span.1 = time as u32;
            }
        }
        let rows = spans
            .into_iter()
            .map(|(label, (begin, end))| {
                TrackRow::new(label, begin, end, parents.get(&label).copied().unwrap_or(0))
            })
            .collect::<Vec<_>>();
        let truth =
            Truth::new(frames, rows).expect("a generated phantom is a consistent reference");
        Self { truth, detections }
    }

    pub fn truth(&self) -> &Truth {
        &self.truth
    }

    pub fn detections(&self) -> &[Vec<TrackDetection>] {
        &self.detections
    }

    pub fn into_parts(self) -> (Truth, Vec<Vec<TrackDetection>>) {
        (self.truth, self.detections)
    }
}

const SPECTRUM_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

fn degrade(spec: Spec, detections: &mut [Vec<TrackDetection>], next: &mut u32) {
    let mut rng = StdRng::seed_from_u64(spec.seed ^ SPECTRUM_SEED);
    for frame in detections.iter_mut() {
        frame.retain(|_| rng.random::<f32>() >= spec.miss);
        if rng.random::<f32>() < spec.false_alarm {
            let scale = scale_of(&mut rng, spec);
            let truth = radii(spec, scale);
            let position = spawn(&mut rng, spec.extent, truth);
            frame.push(measure(
                &mut rng,
                spec,
                ObjectId::new(*next),
                position,
                truth,
            ));
            *next += 1;
        }
    }
}

fn spawn(rng: &mut StdRng, extent: [f32; 3], radii: [f32; 3]) -> [f32; 3] {
    let margin = [0, 1, 2].map(|axis| 3.0 * radii[axis]);
    [0, 1, 2].map(|axis| {
        rng.random_range(margin[axis]..=(extent[axis] - margin[axis]).max(margin[axis]))
    })
}

fn birthplace(rng: &mut StdRng, spec: Spec, live: &[Cell], born: [f32; 3]) -> [f32; 3] {
    if live.is_empty() || rng.random::<f32>() >= spec.crowded {
        return spawn(rng, spec.extent, born);
    }
    let host = live[rng.random_range(0..live.len())];
    let direction = drift(rng, 1.0);
    let length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if length <= f32::EPSILON {
        return host.position;
    }
    let reach = 2.5 * (born[0] + radii(spec, host.scale)[0]);
    let mut position = host.position;
    for axis in 0..3 {
        let margin = 3.0 * born[axis];
        position[axis] = (host.position[axis] + direction[axis] / length * reach)
            .clamp(margin, (spec.extent[axis] - margin).max(margin));
    }
    position
}

fn measure(
    rng: &mut StdRng,
    spec: Spec,
    id: ObjectId,
    position: [f32; 3],
    radii: [f32; 3],
) -> TrackDetection {
    let centroid = [0, 1, 2]
        .map(|axis| position[axis] + rng.random_range(-1.0..=1.0) * spec.noise * radii[axis]);
    let sized = radii.map(|radius| radius * (1.0 + rng.random_range(-spec.noise..=spec.noise)));
    TrackDetection::new(id, centroid, sized)
}

fn scale_of(rng: &mut StdRng, spec: Spec) -> f32 {
    if spec.spread == 0.0 {
        return 1.0;
    }
    (gaussian(rng) * spec.spread).exp()
}

fn rate_of(rng: &mut StdRng, spec: Spec) -> f32 {
    rng.random_range(0.0..=spec.growth)
}

fn gaussian(rng: &mut StdRng) -> f32 {
    let first: f32 = rng.random_range(f32::MIN_POSITIVE..=1.0);
    let second: f32 = rng.random_range(0.0..=1.0);
    (-2.0 * first.ln()).sqrt() * (2.0 * std::f32::consts::PI * second).cos()
}

fn drift(rng: &mut StdRng, step: f32) -> [f32; 3] {
    [0, 1, 2].map(|_| rng.random_range(-step..=step))
}

fn advance(rng: &mut StdRng, spec: Spec, live: &mut [Cell]) {
    for cell in live.iter_mut() {
        let jitter = drift(rng, 0.25 * spec.step);
        let cell_radii = radii(spec, cell.scale);
        let mut moved = [0.0; 3];
        for axis in 0..3 {
            let margin = 3.0 * cell_radii[axis];
            let high = spec.extent[axis] - margin;
            let mut at = cell.position[axis] + cell.velocity[axis] + jitter[axis];
            if at < margin || at > high {
                cell.velocity[axis] = -cell.velocity[axis];
                at = at.clamp(margin, high.max(margin));
            }
            moved[axis] = at;
        }
        cell.position = moved;
    }
    if spec.swirl != 0.0 {
        let centre = [spec.extent[0] * 0.5, spec.extent[1] * 0.5];
        let cosine = spec.swirl.cos();
        let sine = spec.swirl.sin();
        for cell in live.iter_mut() {
            let x = cell.position[0] - centre[0];
            let y = cell.position[1] - centre[1];
            cell.position[0] = centre[0] + x * cosine - y * sine;
            cell.position[1] = centre[1] + x * sine + y * cosine;
            let vx = cell.velocity[0];
            let vy = cell.velocity[1];
            cell.velocity[0] = vx * cosine - vy * sine;
            cell.velocity[1] = vx * sine + vy * cosine;
        }
    }
    if spec.growth > 0.0 {
        for cell in live.iter_mut() {
            cell.scale *= (1.0 + cell.rate).cbrt();
        }
    }
}

fn divide(
    rng: &mut StdRng,
    spec: Spec,
    live: &mut Vec<Cell>,
    parents: &mut BTreeMap<u32, u32>,
    next: &mut u32,
) {
    let dividing = (0..live.len())
        .filter(|_| rng.random::<f32>() < spec.division)
        .take(spec.capacity.saturating_sub(live.len()) / 2)
        .collect::<Vec<_>>();
    for index in dividing.into_iter().rev() {
        let parent = live.swap_remove(index);
        let scale = parent.scale / 2.0f32.cbrt();
        let daughter_radii = radii(spec, scale);
        let axis = rng.random_range(0..3);
        for side in [-1.0f32, 1.0] {
            let label = *next;
            *next += 1;
            parents.insert(label, parent.label);
            let mut position = parent.position;
            let mut velocity = parent.velocity;
            position[axis] = (parent.position[axis] + side * daughter_radii[axis]).clamp(
                daughter_radii[axis],
                spec.extent[axis] - daughter_radii[axis],
            );
            velocity[axis] += side * 0.3 * spec.step;
            live.push(Cell {
                label,
                position,
                velocity,
                scale,
                rate: rate_of(rng, spec),
                siblings: parent.label,
            });
        }
    }
}

fn retire(rng: &mut StdRng, spec: Spec, live: &mut Vec<Cell>) {
    let doomed = (0..live.len())
        .filter(|_| rng.random::<f32>() < spec.death)
        .collect::<Vec<_>>();
    for index in doomed.into_iter().rev() {
        live.swap_remove(index);
    }
}

fn separate(spec: Spec, live: &mut [Cell]) {
    for _ in 0..8 {
        let mut moved = false;
        for first in 0..live.len() {
            for second in first + 1..live.len() {
                let offset =
                    [0, 1, 2].map(|axis| live[second].position[axis] - live[first].position[axis]);
                let distance = offset
                    .into_iter()
                    .map(|value| value * value)
                    .sum::<f32>()
                    .sqrt();
                if live[first].siblings != 0 && live[first].siblings == live[second].siblings {
                    continue;
                }
                let minimum = 1.05
                    * (equivalent(radii(spec, live[first].scale))
                        + equivalent(radii(spec, live[second].scale)));
                if distance >= minimum {
                    continue;
                }
                let axis = (0..3)
                    .max_by(|left, right| offset[*left].abs().total_cmp(&offset[*right].abs()))
                    .expect("a cell offset spans three axes");
                let push = 0.5 * (minimum - distance);
                let sign = if offset[axis] < 0.0 { -1.0 } else { 1.0 };
                for (cell, side) in [(first, -sign), (second, sign)] {
                    let margin = radii(spec, live[cell].scale)[axis];
                    live[cell].position[axis] = (live[cell].position[axis] + side * push)
                        .clamp(margin, (spec.extent[axis] - margin).max(margin));
                }
                moved = true;
            }
        }
        if !moved {
            return;
        }
    }
}

fn equivalent(radii: [f32; 3]) -> f32 {
    (radii[0] * radii[1] * radii[2]).cbrt()
}
