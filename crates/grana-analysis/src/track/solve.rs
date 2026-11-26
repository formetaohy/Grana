use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::TrackParams;

use super::calibration::{self, Family, Hypothesis};
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
        let resolved = resolve(&hypotheses, node_count);
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

fn resolve(hypotheses: &[Hypothesis], node_count: usize) -> Vec<(usize, f64)> {
    let unweighted = hypotheses
        .iter()
        .map(|hypothesis| Entry {
            nodes: hypothesis.nodes,
            count: hypothesis.count,
            weight: 1.0,
        })
        .collect::<Vec<_>>();
    let mut held = Vec::new();
    for group in packing::components(&unweighted, node_count) {
        held.extend(settle(hypotheses, &group, node_count));
    }
    held.sort_unstable_by_key(|(index, _)| *index);
    held
}

fn settle(hypotheses: &[Hypothesis], group: &[usize], node_count: usize) -> Vec<(usize, f64)> {
    let mut nodes = group
        .iter()
        .flat_map(|index| hypotheses[*index].nodes[..hypotheses[*index].count as usize].iter())
        .map(|node| *node as usize)
        .collect::<Vec<_>>();
    nodes.sort_unstable();
    nodes.dedup();
    let mut incidents = vec![Vec::new(); node_count];
    for (position, &index) in group.iter().enumerate() {
        for node in &hypotheses[index].nodes[..hypotheses[index].count as usize] {
            incidents[*node as usize].push(position as u32);
        }
    }
    let marginals = calibration::marginals(hypotheses, group, &incidents, &nodes, node_count);
    let entries = group
        .iter()
        .map(|&index| Entry {
            nodes: hypotheses[index].nodes,
            count: hypotheses[index].count,
            weight: calibration::selection(&hypotheses[index], marginals.probability(index)),
        })
        .collect::<Vec<_>>();
    let incidence = packing::Incidence::new(&entries, node_count);
    let members = (0..entries.len()).collect::<Vec<_>>();
    incidence
        .pack(&entries, &members, node_count)
        .into_iter()
        .map(|position| {
            let index = group[position];
            (index, marginals.probability(index))
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
