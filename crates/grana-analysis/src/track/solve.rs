use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::TrackParams;

use super::candidate::{self, Choice};
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
        let entries = found
            .iter()
            .map(|candidate| match candidate.choice {
                Choice::Link { source, target } => Entry {
                    nodes: [node(node_of[&source]), node(slot[&target]), 0],
                    count: 2,
                    weight: candidate.weight,
                },
                Choice::Divide {
                    source,
                    first,
                    second,
                } => Entry {
                    nodes: [
                        node(node_of[&source]),
                        node(slot[&first]),
                        node(slot[&second]),
                    ],
                    count: 3,
                    weight: candidate.weight,
                },
                Choice::Merge {
                    first,
                    second,
                    target,
                } => Entry {
                    nodes: [
                        node(node_of[&first]),
                        node(node_of[&second]),
                        node(slot[&target]),
                    ],
                    count: 3,
                    weight: candidate.weight,
                },
            })
            .collect::<Vec<_>>();
        let created = chains.len();
        let chosen = packing::pack(&entries, sources.len() + targets.len());
        let mut claimed = vec![false; targets.len()];
        for index in chosen {
            match found[index].choice {
                Choice::Link { source, target } => {
                    claimed[target - targets.start] = true;
                    link(&mut chains, detections, source, target, time);
                }
                Choice::Divide {
                    source,
                    first,
                    second,
                } => {
                    claimed[first - targets.start] = true;
                    claimed[second - targets.start] = true;
                    divide(&mut chains, detections, source, first, second, time);
                }
                Choice::Merge {
                    first,
                    second,
                    target,
                } => {
                    claimed[target - targets.start] = true;
                    merge(&mut chains, detections, first, second, target, time);
                }
            }
        }
        for (position, point) in targets.enumerate() {
            if !claimed[position] {
                chains.push(point, time, Vec3::ZERO);
            }
        }
        let mut next = Vec::with_capacity(sources.len() + chains.len() - created);
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

fn node(index: usize) -> u32 {
    u32::try_from(index).expect("a track boundary holds fewer than 2^32 chains and objects")
}

fn link(chains: &mut Chains, detections: &Detections, source: usize, target: usize, time: u32) {
    let last = chains.last(source);
    let elapsed = f32::max((time - detections.frame(last)) as f32, 1.0e-6);
    let velocity = (detections.centroid(target) - detections.centroid(last)) / elapsed;
    if chains.chain(source).end() + 1 == time {
        chains.extend(source, target, time, velocity);
        return;
    }
    let child = chains.push(target, time, velocity);
    chains.attach(source, child);
}

fn divide(
    chains: &mut Chains,
    detections: &Detections,
    source: usize,
    first: usize,
    second: usize,
    time: u32,
) {
    let last = chains.last(source);
    let elapsed = f32::max((time - detections.frame(last)) as f32, 1.0e-6);
    let position = detections.centroid(last);
    for daughter in [first, second] {
        let velocity = (detections.centroid(daughter) - position) / elapsed;
        let child = chains.push(daughter, time, velocity);
        chains.attach(source, child);
    }
}

fn merge(
    chains: &mut Chains,
    detections: &Detections,
    first: usize,
    second: usize,
    target: usize,
    time: u32,
) {
    let left = detections.centroid(chains.last(first));
    let right = detections.centroid(chains.last(second));
    let velocity = detections.centroid(target) - (left + right) * 0.5;
    let child = chains.push(target, time, velocity);
    chains.attach(first, child);
    chains.attach(second, child);
}
