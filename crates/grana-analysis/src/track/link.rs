use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::TrackParams;

use super::assign::{self, Edge};
use super::detection::Detections;

const PASSES: usize = 3;
const DUMMY_COST: f64 = 1.0;
const GAP_COST: f64 = 0.25;

pub(super) struct Matching {
    successor: Vec<Option<usize>>,
    predecessor: Vec<Option<usize>>,
}

impl Matching {
    fn empty(count: usize) -> Self {
        Self {
            successor: vec![None; count],
            predecessor: vec![None; count],
        }
    }

    pub fn successor(&self, index: usize) -> Option<usize> {
        self.successor[index]
    }

    pub fn predecessor(&self, index: usize) -> Option<usize> {
        self.predecessor[index]
    }
}

impl PartialEq for Matching {
    fn eq(&self, other: &Self) -> bool {
        self.successor == other.successor
    }
}

pub(super) fn link(
    detections: &Detections,
    params: TrackParams,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<Matching> {
    let count = detections.len();
    if count == 0 {
        return Some(Matching::empty(count));
    }
    let total = 2 * detections.frame_count();
    let mut velocity = vec![Vec3::ZERO; count];
    let mut matching = Matching::empty(count);
    for pass in 0..PASSES {
        let mut report = |frame: usize| progress(frame, total);
        let rows = candidates(
            detections,
            params,
            &velocity,
            cancel,
            if pass == 0 { Some(&mut report) } else { None },
        )?;
        let assignment = assign::solve(&rows, 2 * count, cancel)?;
        let next = matched(&assignment, count);
        if next == matching {
            break;
        }
        velocity = motion(detections, &next);
        matching = next;
    }
    Some(matching)
}

fn matched(assignment: &[usize], count: usize) -> Matching {
    let mut matching = Matching::empty(count);
    for (source, &column) in assignment.iter().enumerate() {
        if column >= count {
            continue;
        }
        matching.successor[source] = Some(column);
        matching.predecessor[column] = Some(source);
    }
    matching
}

fn motion(detections: &Detections, matching: &Matching) -> Vec<Vec3> {
    (0..detections.len())
        .map(|index| {
            let Some(previous) = matching.predecessor(index) else {
                return Vec3::ZERO;
            };
            let elapsed = (detections.frame(index) - detections.frame(previous)) as f32;
            (detections.centroid(index) - detections.centroid(previous)) / elapsed
        })
        .collect()
}

fn candidates(
    detections: &Detections,
    params: TrackParams,
    velocity: &[Vec3],
    cancel: &AtomicBool,
    mut progress: Option<&mut dyn FnMut(usize)>,
) -> Option<Vec<Vec<Edge>>> {
    let count = detections.len();
    let step = params.max_step();
    let span = params.max_gap() as usize + 1;
    let mut rows = vec![Vec::new(); count];
    for frame in 0..detections.frame_count() {
        if let Some(report) = progress.as_deref_mut() {
            report(frame);
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        for reach in 1..=span {
            let target = frame + reach;
            if target >= detections.frame_count() {
                break;
            }
            let reach_gate = step * reach as f32;
            let grid = detections.grid(target, reach_gate);
            for source in detections.span(frame) {
                let predicted = detections.centroid(source) + velocity[source] * reach as f32;
                let gate = detections.gate(source, reach_gate);
                grid.near(predicted, gate, |candidate, distance| {
                    let closeness = 1.0 - distance;
                    if closeness <= 0.0 {
                        return;
                    }
                    rows[source].push(Edge {
                        column: candidate,
                        cost: 1.0 - f64::from(closeness) + GAP_COST * (reach as f64 - 1.0),
                    });
                });
            }
        }
        for source in detections.span(frame) {
            rows[source].push(Edge {
                column: count + source,
                cost: DUMMY_COST,
            });
        }
    }
    Some(rows)
}
