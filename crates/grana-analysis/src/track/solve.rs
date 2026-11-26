use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::TrackParams;

use super::calibration::{self, Calibration, Family, Hypothesis};
use super::candidate::{self, Candidate, Choice};
use super::chain::Chains;
use super::detection::Detections;
use super::packing::{self, Entry};

pub(super) fn solve(
    detections: &Detections,
    params: TrackParams,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<Chains> {
    let frames = detections.frame_count();
    let span = params.max_gap() + 1;
    let mut chains = Chains::new();
    let mut open = Vec::<usize>::new();
    for frame in 0..frames {
        progress(frame, frames);
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let time = frame as u32;
        let targets = detections.span(frame);
        if frame == 0 {
            for point in targets {
                open.push(chains.push(point, time, Vec3::ZERO));
            }
            continue;
        }
        let sources = std::mem::take(&mut open);
        let node_of = sources
            .iter()
            .enumerate()
            .map(|(node, &chain)| (chain, node))
            .collect::<HashMap<usize, usize>>();
        let mut slot = HashMap::with_capacity(targets.len());
        for (position, point) in targets.clone().enumerate() {
            slot.insert(point, sources.len() + position);
        }
        let found = candidate::candidates(detections, &chains, params, frame, &sources);
        let hypotheses = hypotheses_of(&found, &node_of, &slot);
        let node_count = sources.len() + targets.len();
        let mut calibration = Calibration::new(node_count);
        let weight = calibration.weigh(&hypotheses);
        let resolved = resolve(&hypotheses, &weight, &mut calibration, node_count);
        let created = chains.len();
        let mut claimed = vec![false; targets.len()];
        for (index, confidence) in resolved {
            match found[index].choice {
                Choice::Link { source, target } => {
                    claimed[target - targets.start] = true;
                    link(&mut chains, detections, source, target, time, confidence);
                }
                Choice::Divide {
                    source,
                    first,
                    second,
                } => {
                    claimed[first - targets.start] = true;
                    claimed[second - targets.start] = true;
                    divide(
                        &mut chains,
                        detections,
                        source,
                        first,
                        second,
                        time,
                        confidence,
                    );
                }
                Choice::Merge {
                    first,
                    second,
                    target,
                } => {
                    claimed[target - targets.start] = true;
                    merge(
                        &mut chains,
                        detections,
                        first,
                        second,
                        target,
                        time,
                        confidence,
                    );
                }
            }
        }
        for (position, point) in targets.enumerate() {
            if !claimed[position] {
                chains.push(point, time, Vec3::ZERO);
            }
        }
        let mut next = Vec::with_capacity(sources.len());
        for chain in sources {
            let held = chains.chain(chain);
            if held.outgoing() == 0 && held.end() + span > time {
                next.push(chain);
            }
        }
        next.extend(created..chains.len());
        open = next;
    }
    progress(frames, frames);
    Some(chains)
}

fn hypotheses_of(
    found: &[Candidate],
    node_of: &HashMap<usize, usize>,
    slot: &HashMap<usize, usize>,
) -> Vec<Hypothesis> {
    found
        .iter()
        .map(|candidate| {
            let (nodes, count, edges, family) = match candidate.choice {
                Choice::Link { source, target } => (
                    [node(node_of[&source]), node(slot[&target]), 0],
                    2,
                    1,
                    Family::Link,
                ),
                Choice::Divide {
                    source,
                    first,
                    second,
                } => (
                    [
                        node(node_of[&source]),
                        node(slot[&first]),
                        node(slot[&second]),
                    ],
                    3,
                    2,
                    Family::Divide,
                ),
                Choice::Merge {
                    first,
                    second,
                    target,
                } => (
                    [
                        node(node_of[&first]),
                        node(node_of[&second]),
                        node(slot[&target]),
                    ],
                    3,
                    2,
                    Family::Merge,
                ),
            };
            Hypothesis {
                nodes,
                count,
                edges,
                family,
                score: candidate.score,
                gap: candidate.gap,
            }
        })
        .collect()
}

fn resolve(
    hypotheses: &[Hypothesis],
    weight: &[f64],
    calibration: &mut Calibration,
    node_count: usize,
) -> Vec<(usize, f64)> {
    let entries = hypotheses
        .iter()
        .zip(weight)
        .map(|(hypothesis, held)| entry(hypothesis, *held))
        .collect::<Vec<_>>();
    let incidence = packing::Incidence::new(&entries, node_count);
    let mut held = Vec::new();
    for group in packing::components(&entries, node_count) {
        held.extend(settle(
            hypotheses,
            &entries,
            &incidence,
            weight,
            &group,
            calibration,
            node_count,
        ));
    }
    held.sort_unstable_by_key(|(index, _)| *index);
    held
}

fn entry(hypothesis: &Hypothesis, weight: f64) -> Entry {
    Entry {
        nodes: hypothesis.nodes,
        count: hypothesis.count,
        weight: calibration::selection(hypothesis, weight),
    }
}

fn settle(
    hypotheses: &[Hypothesis],
    entries: &[Entry],
    incidence: &packing::Incidence,
    weight: &[f64],
    group: &[usize],
    calibration: &mut Calibration,
    node_count: usize,
) -> Vec<(usize, f64)> {
    let chosen = incidence.pack(entries, group, node_count);
    if !group
        .iter()
        .any(|index| calibration::abstains(&hypotheses[*index], weight[*index]))
    {
        return chosen
            .into_iter()
            .map(|index| (index, calibration::posterior(weight[index])))
            .collect();
    }
    let survivors = group
        .iter()
        .copied()
        .filter(|index| !calibration::abstains(&hypotheses[*index], weight[*index]))
        .collect::<Vec<_>>();
    let refined = calibration.weigh(
        &survivors
            .iter()
            .map(|index| hypotheses[*index])
            .collect::<Vec<_>>(),
    );
    let refined_entries = survivors
        .iter()
        .zip(&refined)
        .map(|(index, held)| entry(&hypotheses[*index], *held))
        .collect::<Vec<_>>();
    let refined_incidence = packing::Incidence::new(&refined_entries, node_count);
    let refined_members = (0..refined_entries.len()).collect::<Vec<_>>();
    refined_incidence
        .pack(&refined_entries, &refined_members, node_count)
        .into_iter()
        .map(|position| {
            (
                survivors[position],
                calibration::posterior(refined[position]),
            )
        })
        .collect()
}

fn node(index: usize) -> u32 {
    u32::try_from(index).expect("a track boundary holds fewer than 2^32 chains and objects")
}

fn link(
    chains: &mut Chains,
    detections: &Detections,
    source: usize,
    target: usize,
    time: u32,
    confidence: f64,
) {
    let last = chains.last(source);
    let elapsed = f32::max((time - detections.frame(last)) as f32, 1.0e-6);
    let velocity = (detections.centroid(target) - detections.centroid(last)) / elapsed;
    if chains.chain(source).end() + 1 == time {
        chains.extend(source, target, time, velocity);
        return;
    }
    let child = chains.push(target, time, velocity);
    chains.attach(source, child, confidence);
}

fn divide(
    chains: &mut Chains,
    detections: &Detections,
    source: usize,
    first: usize,
    second: usize,
    time: u32,
    confidence: f64,
) {
    let last = chains.last(source);
    let elapsed = f32::max((time - detections.frame(last)) as f32, 1.0e-6);
    let position = detections.centroid(last);
    for daughter in [first, second] {
        let velocity = (detections.centroid(daughter) - position) / elapsed;
        let child = chains.push(daughter, time, velocity);
        chains.attach(source, child, confidence);
    }
}

fn merge(
    chains: &mut Chains,
    detections: &Detections,
    first: usize,
    second: usize,
    target: usize,
    time: u32,
    confidence: f64,
) {
    let left = detections.centroid(chains.last(first));
    let right = detections.centroid(chains.last(second));
    let velocity = detections.centroid(target) - (left + right) * 0.5;
    let child = chains.push(target, time, velocity);
    chains.attach(first, child, confidence);
    chains.attach(second, child, confidence);
}
