use std::collections::BTreeMap;

use grana_object::TrackDetection;
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
        }
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
    radii: [f32; 3],
    siblings: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Phantom {
    truth: Truth,
    detections: Vec<Vec<TrackDetection>>,
}

impl Phantom {
    pub fn of(spec: Spec) -> Self {
        let mut rng = StdRng::seed_from_u64(spec.seed);
        let radii = [spec.radius, spec.radius, spec.radius * spec.anisotropy];
        let mut live = (0..spec.cells)
            .map(|index| {
                let label = index as u32 + 1;
                Cell {
                    label,
                    position: spawn(&mut rng, spec.extent, radii),
                    velocity: drift(&mut rng, spec.step),
                    radii,
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
                markers.push(Marker::new(cell.label, time, cell.position, cell.radii));
                if rng.random::<f32>() >= spec.miss {
                    found.push(TrackDetection::new(cell.position, cell.radii));
                }
            }
            if rng.random::<f32>() < spec.false_alarm {
                found.push(TrackDetection::new(
                    spawn(&mut rng, spec.extent, radii),
                    radii,
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
                live.push(Cell {
                    label: next,
                    position: spawn(&mut rng, spec.extent, radii),
                    velocity: drift(&mut rng, spec.step),
                    radii,
                    siblings: 0,
                });
                next += 1;
            }
            separate(spec.extent, &mut live);
            for cell in &mut live {
                cell.siblings = 0;
            }
            advance(&mut rng, spec, &mut live);
        }
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

fn spawn(rng: &mut StdRng, extent: [f32; 3], radii: [f32; 3]) -> [f32; 3] {
    let margin = [0, 1, 2].map(|axis| 3.0 * radii[axis]);
    [0, 1, 2].map(|axis| {
        rng.random_range(margin[axis]..=(extent[axis] - margin[axis]).max(margin[axis]))
    })
}

fn drift(rng: &mut StdRng, step: f32) -> [f32; 3] {
    [0, 1, 2].map(|_| rng.random_range(-step..=step))
}

fn advance(rng: &mut StdRng, spec: Spec, live: &mut [Cell]) {
    for cell in live.iter_mut() {
        let jitter = drift(rng, 0.25 * spec.step);
        let mut moved = [0.0; 3];
        for axis in 0..3 {
            let margin = 3.0 * cell.radii[axis];
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
        let radii = parent.radii.map(|value| value / 2.0f32.cbrt());
        let axis = rng.random_range(0..3);
        for side in [-1.0f32, 1.0] {
            let label = *next;
            *next += 1;
            parents.insert(label, parent.label);
            let mut position = parent.position;
            let mut velocity = parent.velocity;
            position[axis] = (parent.position[axis] + side * radii[axis])
                .clamp(radii[axis], spec.extent[axis] - radii[axis]);
            velocity[axis] += side * 0.3 * spec.step;
            live.push(Cell {
                label,
                position,
                velocity,
                radii,
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

fn separate(extent: [f32; 3], live: &mut [Cell]) {
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
                let minimum =
                    1.05 * (equivalent(live[first].radii) + equivalent(live[second].radii));
                if distance >= minimum {
                    continue;
                }
                let axis = (0..3)
                    .max_by(|left, right| offset[*left].abs().total_cmp(&offset[*right].abs()))
                    .expect("a cell offset spans three axes");
                let push = 0.5 * (minimum - distance);
                let sign = if offset[axis] < 0.0 { -1.0 } else { 1.0 };
                for (cell, side) in [(first, -sign), (second, sign)] {
                    let margin = live[cell].radii[axis];
                    live[cell].position[axis] = (live[cell].position[axis] + side * push)
                        .clamp(margin, (extent[axis] - margin).max(margin));
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
