use std::collections::HashMap;

use glam::Vec3;
use grana_object::TrackParams;

use super::chain::Chains;
use super::detection::Detections;

const CONFIDENCE_CEILING: f64 = 0.98;
const LINK_GAIN: f64 = 0.45;
const DIVISION_GAIN: f64 = 0.45;
const MERGE_GAIN: f64 = 0.45;
const DIVISION_SEPARATION: f64 = 1.0;
const DIVISION_SPREAD: f64 = 0.9;
const DIVISION_VOLUME_SPREAD: f64 = 1.0;
const MERGE_OVERLAP: f64 = 0.3;
const MERGE_SPREAD: f64 = 0.3;
const MERGE_VOLUME_SPREAD: f64 = 0.6;
const GAP_SHARE: f64 = 0.5;
const LINK_ZONE: f64 = 0.7;
const PAIRS: usize = 24;
const TARGET_WEIGHT: f64 = 1.0;
const EVENT_FLOOR: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Link {
        source: usize,
        target: usize,
    },
    Divide {
        source: usize,
        first: usize,
        second: usize,
    },
    Merge {
        first: usize,
        second: usize,
        target: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub(super) choice: Choice,
    pub(super) weight: f64,
}

pub(super) fn predict(detections: &Detections, chains: &Chains, index: usize, time: u32) -> Vec3 {
    let chain = chains.chain(index);
    let elapsed = (time - chain.end()) as f32;
    detections.centroid(chain.last()) + chain.velocity() * elapsed
}

pub(super) fn candidates(
    detections: &Detections,
    chains: &Chains,
    params: TrackParams,
    frame: usize,
    open: &[usize],
) -> Vec<Candidate> {
    let time = frame as u32;
    let step = params.max_step();
    if open.is_empty() || detections.span(frame).is_empty() {
        return Vec::new();
    }
    let grid = detections.grid(frame, step);
    let mut reachable = HashMap::<usize, Vec<(usize, f32)>>::new();
    let mut found = Vec::new();
    for &source in open {
        let chain = chains.chain(source);
        let reach = time - chain.end();
        let predicted = predict(detections, chains, source, time);
        let gate = detections.gate(chain.last(), reach as f32, step);
        let motion = steadiness(detections, chains.points(source));
        grid.near(predicted, gate, |target, distance| {
            reachable
                .entry(target)
                .or_default()
                .push((source, distance));
            let score = link_score(distance) * motion * gap_share(reach);
            found.extend(weighted(
                Choice::Link { source, target },
                score,
                LINK_GAIN,
                1,
            ));
        });
    }
    if params.events().division() {
        divisions(detections, chains, open, &grid, params, frame, &mut found);
    }
    if params.events().merge() {
        merges(detections, chains, &reachable, params, time, &mut found);
    }
    found
}

fn divisions(
    detections: &Detections,
    chains: &Chains,
    open: &[usize],
    grid: &super::detection::Grid<'_>,
    params: TrackParams,
    frame: usize,
    found: &mut Vec<Candidate>,
) {
    let time = frame as u32;
    let step = params.max_step();
    let range = params.events().division_range();
    for &source in open {
        let chain = chains.chain(source);
        let reach = time - chain.end();
        let predicted = predict(detections, chains, source, time);
        let last = chain.last();
        let position = detections.centroid(last);
        let radius = detections.radius(last);
        let gate = detections.gate(last, reach as f32, step) + Vec3::splat(range);
        let mut nearby = Vec::new();
        grid.near(predicted, gate, |target, distance| {
            nearby.push((distance, target))
        });
        nearby
            .sort_unstable_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
        nearby.truncate(PAIRS);
        let nearby = nearby
            .into_iter()
            .map(|(_, target)| target)
            .collect::<Vec<_>>();
        let motion = steadiness(detections, chains.points(source));
        for first in 0..nearby.len() {
            for second in first + 1..nearby.len() {
                let daughter = nearby[first];
                let other = nearby[second];
                let near = detections.centroid(daughter);
                let seat = detections.centroid(other);
                if !within(near, position, range) || !within(seat, position, range) {
                    continue;
                }
                let midpoint = (near + seat) * 0.5;
                let middle = centred(position, midpoint, radius);
                if middle <= 0.0 {
                    continue;
                }
                let offset = Vec3::distance(near, seat);
                let reach = detections.radius(daughter) + detections.radius(other);
                if !(0.2..=1.8).contains(&(offset / reach)) {
                    continue;
                }
                let opening = separation(
                    offset,
                    detections.radius(daughter),
                    detections.radius(other),
                );
                let volume = conserved(
                    volume(radius),
                    volume(detections.radius(daughter)) + volume(detections.radius(other)),
                    DIVISION_VOLUME_SPREAD,
                );
                let score =
                    middle * opening * volume * motion * reached(predicted, midpoint, range);
                if score < EVENT_FLOOR {
                    continue;
                }
                found.extend(weighted(
                    Choice::Divide {
                        source,
                        first: daughter,
                        second: other,
                    },
                    score,
                    DIVISION_GAIN,
                    2,
                ));
            }
        }
    }
}

fn merges(
    detections: &Detections,
    chains: &Chains,
    reachable: &HashMap<usize, Vec<(usize, f32)>>,
    params: TrackParams,
    time: u32,
    found: &mut Vec<Candidate>,
) {
    let overlap_gate = f64::from(params.events().merge_overlap());
    let range = params.events().merge_range();
    for (&target, sources) in reachable {
        if sources.len() < 2 {
            continue;
        }
        let mut members = sources.clone();
        members
            .sort_unstable_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)));
        members.truncate(PAIRS);
        let seat = detections.centroid(target);
        let radius = detections.radius(target);
        for first in 0..members.len() {
            for second in first + 1..members.len() {
                let (left, _) = members[first];
                let (right, _) = members[second];
                if chains.chain(left).end() + 1 != time || chains.chain(right).end() + 1 != time {
                    continue;
                }
                let left_last = chains.last(left);
                let right_last = chains.last(right);
                let left_position = detections.centroid(left_last);
                let right_position = detections.centroid(right_last);
                if !within(left_position, seat, range) || !within(right_position, seat, range) {
                    continue;
                }
                let overlap = overlapping(
                    left_position,
                    detections.radius(left_last),
                    right_position,
                    detections.radius(right_last),
                );
                if overlap < overlap_gate {
                    continue;
                }
                let fusion = fused(overlap);
                if fusion <= 0.0 {
                    continue;
                }
                let volume = conserved(
                    volume(detections.radius(left_last)) + volume(detections.radius(right_last)),
                    volume(radius),
                    MERGE_VOLUME_SPREAD,
                );
                let middle = centred(seat, (left_position + right_position) * 0.5, radius);
                if middle <= 0.0 {
                    continue;
                }
                let motion = steadiness(detections, chains.points(left))
                    .min(steadiness(detections, chains.points(right)));
                let score = fusion * volume * middle * motion;
                if score < EVENT_FLOOR {
                    continue;
                }
                found.extend(weighted(
                    Choice::Merge {
                        first: left,
                        second: right,
                        target,
                    },
                    score,
                    MERGE_GAIN,
                    1,
                ));
            }
        }
    }
}

fn weighted(choice: Choice, score: f64, gain: f64, covered: u8) -> Option<Candidate> {
    let confidence = (0.5 + gain * score.clamp(0.0, 1.0)).min(CONFIDENCE_CEILING);
    let odds = (confidence / (1.0 - confidence)).ln();
    (odds > 0.0).then_some(Candidate {
        choice,
        weight: odds + TARGET_WEIGHT * f64::from(covered),
    })
}

fn gap_share(reach: u32) -> f64 {
    1.0 / (1.0 + GAP_SHARE * f64::from(reach - 1))
}

fn link_score(distance: f32) -> f64 {
    let reach = f64::from(distance) / LINK_ZONE;
    (1.0 - reach * reach).clamp(0.0, 1.0)
}

fn volume(radius: f32) -> f64 {
    let radius = f64::from(radius).max(1.0e-6);
    4.0 / 3.0 * std::f64::consts::PI * radius * radius * radius
}

fn within(position: Vec3, at: Vec3, range: f32) -> bool {
    Vec3::distance(position, at) <= range
}

fn centred(position: Vec3, midpoint: Vec3, radius: f32) -> f64 {
    let reach = f64::from(radius.max(1.0e-6));
    (1.0 - f64::from(Vec3::distance(position, midpoint)) / reach).clamp(0.0, 1.0)
}

fn reached(predicted: Vec3, at: Vec3, range: f32) -> f64 {
    let range = f64::from(range.max(1.0e-6));
    (1.0 - f64::from(Vec3::distance(predicted, at)) / range).clamp(0.0, 1.0)
}

fn conserved(expected: f64, observed: f64, spread: f64) -> f64 {
    if expected <= 0.0 || observed <= 0.0 {
        return 0.0;
    }
    let ratio = (observed / expected).ln();
    (-0.5 * (ratio / spread).powi(2)).exp()
}

fn separation(distance: f32, first: f32, second: f32) -> f64 {
    let reach = f64::from((first + second).max(1.0e-6));
    let ratio = f64::from(distance) / reach;
    (-0.5 * ((ratio - DIVISION_SEPARATION) / DIVISION_SPREAD).powi(2)).exp()
}

fn overlapping(first: Vec3, first_radius: f32, second: Vec3, second_radius: f32) -> f64 {
    let reach = f64::from((first_radius + second_radius).max(1.0e-6));
    (reach - f64::from(Vec3::distance(first, second))) / reach
}

fn fused(overlap: f64) -> f64 {
    (-0.5 * ((overlap - MERGE_OVERLAP) / MERGE_SPREAD).powi(2)).exp()
}

fn steadiness(detections: &Detections, points: &[usize]) -> f64 {
    if points.len() < 3 {
        return 1.0;
    }
    let count = points.len();
    let first = points[count - 3];
    let middle = points[count - 2];
    let last = points[count - 1];
    let span = |from: usize, to: usize| {
        f32::max(
            (detections.frame(to) - detections.frame(from)) as f32,
            1.0e-6,
        )
    };
    let leaving = (detections.centroid(middle) - detections.centroid(first)) / span(first, middle);
    let arriving = (detections.centroid(last) - detections.centroid(middle)) / span(middle, last);
    let speed = 0.5 * (leaving.length() + arriving.length());
    if speed <= 1.0e-6 {
        return 1.0;
    }
    let jerk = (arriving - leaving).length();
    f64::from(1.0 / (1.0 + jerk / speed))
}
