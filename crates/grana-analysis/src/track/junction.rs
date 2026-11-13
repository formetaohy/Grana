use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::{Link, TrackParams};

use super::chain::Chains;
use super::detection::Detections;

const HORIZON: f64 = 4.0;
const SUPPORT_FLOOR: f64 = 0.25;
const EVIDENCE_FLOOR: f64 = 0.03;
const FORK_VOLUME: (f64, f64) = (0.2, 5.0);
const JOIN_VOLUME: (f64, f64) = (0.6, 1.7);
const VOLUME_SPREAD: f64 = 0.55;
const SEPARATION_MEAN: f64 = 1.4;
const SEPARATION_SPREAD: f64 = 0.85;
const EPSILON: f64 = 1.0e-9;

struct Fork {
    frame: u32,
    parent: usize,
    other: usize,
    child: Option<usize>,
    score: f64,
}

struct Join {
    frame: u32,
    head: usize,
    other: usize,
    child: Option<usize>,
    score: f64,
}

enum Event {
    Fork(Fork),
    Join(Join),
}

impl Event {
    fn score(&self) -> f64 {
        match self {
            Self::Fork(fork) => fork.score,
            Self::Join(join) => join.score,
        }
    }

    fn rank(&self) -> (u8, [usize; 3]) {
        let (kind, first, second, third) = match self {
            Self::Fork(fork) => (0u8, fork.parent, fork.other, fork.child),
            Self::Join(join) => (1u8, join.head, join.other, join.child),
        };
        (kind, [first, second, third.unwrap_or(usize::MAX)])
    }

    fn chains(&self) -> impl Iterator<Item = usize> {
        let (first, second, third) = match self {
            Self::Fork(fork) => (fork.parent, fork.other, fork.child),
            Self::Join(join) => (join.head, join.other, join.child),
        };
        [Some(first), Some(second), third].into_iter().flatten()
    }

    fn apply(&self, chains: &mut Chains) {
        match self {
            Self::Fork(fork) => match fork.child {
                Some(child) => {
                    chains.attach(fork.parent, child, Link::Division);
                    chains.attach(fork.parent, fork.other, Link::Division);
                }
                None => {
                    let tail = chains.cut(fork.parent, fork.frame);
                    chains.attach(fork.parent, tail, Link::Division);
                    chains.attach(fork.parent, fork.other, Link::Division);
                }
            },
            Self::Join(join) => match join.child {
                Some(child) => {
                    chains.attach(join.head, child, Link::Merge);
                    chains.attach(join.other, child, Link::Merge);
                }
                None => {
                    let tail = chains.cut(join.head, join.frame);
                    chains.attach(join.head, tail, Link::Merge);
                    chains.attach(join.other, tail, Link::Merge);
                }
            },
        }
    }
}

pub(super) fn attach(
    chains: &mut Chains,
    detections: &Detections,
    params: TrackParams,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<()> {
    let frames = detections.frame_count();
    let total = 2 * frames;
    let events = params.events();
    let mut support = Support::default();
    for frame in 1..frames {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        progress(frames + frame, total);
        let mut candidates = Vec::new();
        if events.division() {
            forks(
                chains,
                detections,
                &mut support,
                frame,
                events.division_range(),
                &mut candidates,
            );
        }
        if events.merge() {
            joins(
                chains,
                detections,
                &mut support,
                frame,
                events.merge_range(),
                events.merge_overlap(),
                &mut candidates,
            );
        }
        candidates.sort_unstable_by_key(|event| event.rank());
        for index in select(&candidates) {
            candidates[index].apply(chains);
        }
    }
    Some(())
}

#[derive(Clone, Copy)]
struct Evidence {
    stamp: u64,
    length: usize,
    continuation: f64,
}

impl Default for Evidence {
    fn default() -> Self {
        Self {
            stamp: u64::MAX,
            length: 0,
            continuation: 0.5,
        }
    }
}

#[derive(Default)]
struct Support {
    chains: Vec<Evidence>,
}

impl Support {
    fn entry(&mut self, detections: &Detections, chains: &Chains, chain: usize) -> Evidence {
        if self.chains.len() <= chain {
            self.chains.resize(chain + 1, Evidence::default());
        }
        let stamp = chains.stamp(chain);
        if self.chains[chain].stamp != stamp {
            self.chains[chain] = Evidence {
                stamp,
                length: chains.points(chain).len(),
                continuation: continue_score(detections, chains, chain),
            };
        }
        self.chains[chain]
    }

    fn onward(
        &mut self,
        detections: &Detections,
        chains: &Chains,
        chain: usize,
        from: usize,
    ) -> f64 {
        let evidence = self.entry(detections, chains, chain);
        let remaining = evidence.length.saturating_sub(from);
        (SUPPORT_FLOOR + (1.0 - SUPPORT_FLOOR) * persistence(remaining)) * evidence.continuation
    }

    fn established(&mut self, detections: &Detections, chains: &Chains, chain: usize) -> f64 {
        self.onward(detections, chains, chain, 0)
    }
}

fn persistence(frames: usize) -> f64 {
    if frames == 0 {
        return 0.0;
    }
    1.0 - (-(frames as f64) / HORIZON).exp()
}

fn volume(radius: f32) -> f64 {
    f64::from(radius).powi(3)
}

fn conserved(expected: f64, observed: f64) -> f64 {
    if expected <= 0.0 || observed <= 0.0 {
        return 0.0;
    }
    (-0.5 * ((observed / expected).ln() / VOLUME_SPREAD).powi(2)).exp()
}

fn within(ratio: f64, bounds: (f64, f64)) -> bool {
    ratio.is_finite() && (bounds.0..=bounds.1).contains(&ratio)
}

fn separated(distance: f32, first: f32, second: f32) -> f64 {
    let reach = f64::from(first + second).max(EPSILON);
    let ratio = f64::from(distance) / reach;
    (-0.5 * ((ratio - SEPARATION_MEAN) / SEPARATION_SPREAD).powi(2)).exp()
}

fn reached(from: Vec3, to: Vec3, range: f32) -> f64 {
    let ratio = f64::from(from.distance(to)) / f64::from(range.max(f32::EPSILON));
    (1.0 - ratio).clamp(0.0, 1.0)
}

fn together(centre: Vec3, mid: Vec3, radius: f32) -> f64 {
    let ratio = f64::from(centre.distance(mid)) / f64::from(radius.max(f32::EPSILON));
    (1.0 - ratio).clamp(0.0, 1.0)
}

fn continue_score(detections: &Detections, chains: &Chains, chain: usize) -> f64 {
    let points = chains.points(chain);
    if points.len() < 3 {
        return 0.5;
    }
    let mut speed = 0.0f64;
    let mut jerk = 0.0f64;
    let mut previous: Option<Vec3> = None;
    for pair in points.windows(2) {
        let from = detections.centroid(pair[0]);
        let to = detections.centroid(pair[1]);
        let elapsed = (detections.frame(pair[1]) - detections.frame(pair[0])).max(1) as f32;
        let velocity = (to - from) / elapsed;
        speed += f64::from(velocity.length());
        if let Some(previous) = previous {
            jerk += f64::from((velocity - previous).length());
        }
        previous = Some(velocity);
    }
    let segments = (points.len() - 1) as f64;
    let mean = speed / segments;
    if mean <= EPSILON {
        return 0.5;
    }
    (1.0 / (1.0 + jerk / speed)).clamp(0.0, 1.0)
}

fn forks(
    chains: &Chains,
    detections: &Detections,
    support: &mut Support,
    frame: usize,
    range: f32,
    events: &mut Vec<Event>,
) {
    let time = frame as u32;
    let (starts, _) = start_chains(chains, frame);
    let grid = detections.grid(frame, range);
    for parent in 0..chains.len() {
        let chain = chains.chain(parent);
        if chain.start() >= time || chain.end() < time || chain.incoming() > 1 {
            continue;
        }
        let position = (time - chain.start()) as usize;
        let points = chains.points(parent);
        let mother = points[position - 1];
        let carried = points[position];
        let near = detections.centroid(carried);
        let predicted = predict(detections, chains, parent, position - 1, time);
        let mother_volume = volume(detections.radius(mother));
        let carried_volume = volume(detections.radius(carried));
        let mother_support = support.established(detections, chains, parent);
        grid.near(near, Vec3::splat(range), |point, _| {
            let Some(&start) = starts.get(&point) else {
                return;
            };
            let seat = detections.centroid(point);
            let seat_volume = volume(detections.radius(point));
            if !within((carried_volume + seat_volume) / mother_volume, FORK_VOLUME) {
                return;
            }
            let evidence = mother_support
                * support.onward(detections, chains, parent, position)
                * support.established(detections, chains, start);
            if evidence < EVIDENCE_FLOOR {
                return;
            }
            let middle = together(
                detections.centroid(mother),
                (near + seat) * 0.5,
                detections.radius(mother),
            );
            if middle <= 0.0 {
                return;
            }
            let score = reached(predicted, near, range)
                * reached(predicted, seat, range)
                * middle
                * conserved(mother_volume, carried_volume + seat_volume)
                * separated(
                    near.distance(seat),
                    detections.radius(carried),
                    detections.radius(point),
                )
                * evidence;
            events.push(Event::Fork(Fork {
                frame: time,
                parent,
                other: start,
                child: None,
                score,
            }));
        });
    }
    for parent in 0..chains.len() {
        let chain = chains.chain(parent);
        if chain.end() + 1 != time || chain.outgoing() > 0 || chain.incoming() > 1 {
            continue;
        }
        let points = chains.points(parent);
        let mother = *points.last().expect("a chain holds points");
        let predicted = predict(detections, chains, parent, points.len() - 1, time);
        let mother_volume = volume(detections.radius(mother));
        let mother_support = support.established(detections, chains, parent);
        let mut nearby = Vec::new();
        grid.near(predicted, Vec3::splat(range), |point, distance| {
            if let Some(&start) = starts.get(&point) {
                nearby.push((start, point, distance));
            }
        });
        nearby.sort_unstable_by_key(|(start, _, _)| *start);
        for first in 0..nearby.len() {
            for second in first + 1..nearby.len() {
                let (first_start, first_point, first_distance) = nearby[first];
                let (second_start, second_point, second_distance) = nearby[second];
                let first_seat = detections.centroid(first_point);
                let second_seat = detections.centroid(second_point);
                let mid = (first_seat + second_seat) * 0.5;
                let volume_gain = volume(detections.radius(first_point))
                    + volume(detections.radius(second_point));
                if !within(volume_gain / mother_volume, FORK_VOLUME) {
                    continue;
                }
                let evidence = mother_support
                    * support.established(detections, chains, first_start)
                    * support.established(detections, chains, second_start);
                if evidence < EVIDENCE_FLOOR {
                    continue;
                }
                let middle = together(mid, detections.centroid(mother), detections.radius(mother));
                if middle <= 0.0 {
                    continue;
                }
                let score = reached(predicted, mid, range)
                    * ((1.0 - f64::from(first_distance)).max(0.0)
                        * (1.0 - f64::from(second_distance)).max(0.0))
                    .sqrt()
                    * middle
                    * conserved(mother_volume, volume_gain)
                    * separated(
                        first_seat.distance(second_seat),
                        detections.radius(first_point),
                        detections.radius(second_point),
                    )
                    * evidence;
                events.push(Event::Fork(Fork {
                    frame: time,
                    parent,
                    other: second_start,
                    child: Some(first_start),
                    score,
                }));
            }
        }
    }
}

fn joins(
    chains: &Chains,
    detections: &Detections,
    support: &mut Support,
    frame: usize,
    range: f32,
    overlap: f32,
    events: &mut Vec<Event>,
) {
    let time = frame as u32;
    let ends = end_chains(chains, frame);
    let grid = detections.grid(frame - 1, range);
    for head in 0..chains.len() {
        let chain = chains.chain(head);
        if chain.start() >= time || chain.end() < time || chain.outgoing() > 0 {
            continue;
        }
        let position = (time - chain.start()) as usize;
        let points = chains.points(head);
        let carried = points[position];
        let source = points[position - 1];
        let carried_centre = detections.centroid(carried);
        let carried_volume = volume(detections.radius(carried));
        let source_established = support.established(detections, chains, head);
        let merged_support = support.onward(detections, chains, head, position);
        grid.near(carried_centre, Vec3::splat(range), |point, _| {
            let Some(&end) = ends.get(&point) else {
                return;
            };
            let partner = detections.centroid(point);
            if overlap_of(detections, point, source) < overlap {
                return;
            }
            let source_volume =
                volume(detections.radius(source)) + volume(detections.radius(point));
            if !within(carried_volume / source_volume, JOIN_VOLUME) {
                return;
            }
            let evidence = source_established.min(support.established(detections, chains, end))
                * merged_support;
            if evidence < EVIDENCE_FLOOR {
                return;
            }
            let middle = together(
                carried_centre,
                (detections.centroid(source) + partner) * 0.5,
                detections.radius(carried),
            );
            if middle <= 0.0 {
                return;
            }
            let score = middle
                * conserved(source_volume, carried_volume)
                * separated(
                    detections.centroid(source).distance(partner),
                    detections.radius(source),
                    detections.radius(point),
                )
                * evidence;
            events.push(Event::Join(Join {
                frame: time,
                head,
                other: end,
                child: None,
                score,
            }));
        });
    }
    let (_, order) = start_chains(chains, frame);
    for child in order {
        let points = chains.points(child);
        let merged = points[0];
        let seat = detections.centroid(merged);
        let merged_support = support.established(detections, chains, child);
        let mut nearby = Vec::new();
        grid.near(seat, Vec3::splat(range), |point, distance| {
            if let Some(&end) = ends.get(&point) {
                nearby.push((end, point, distance));
            }
        });
        nearby.sort_unstable_by_key(|(end, _, _)| *end);
        for first in 0..nearby.len() {
            for second in first + 1..nearby.len() {
                let (first_end, first_point, first_distance) = nearby[first];
                let (second_end, second_point, second_distance) = nearby[second];
                let first_position = detections.centroid(first_point);
                let second_position = detections.centroid(second_point);
                if overlap_of(detections, first_point, second_point) < overlap {
                    continue;
                }
                let mid = (first_position + second_position) * 0.5;
                let source_volume = volume(detections.radius(first_point))
                    + volume(detections.radius(second_point));
                let merged_volume = volume(detections.radius(merged));
                if !within(merged_volume / source_volume, JOIN_VOLUME) {
                    continue;
                }
                let evidence = merged_support
                    * support.established(detections, chains, first_end)
                    * support.established(detections, chains, second_end);
                if evidence < EVIDENCE_FLOOR {
                    continue;
                }
                let middle = together(seat, mid, detections.radius(merged));
                if middle <= 0.0 {
                    continue;
                }
                let score = reached(seat, mid, range)
                    * ((1.0 - f64::from(first_distance)).max(0.0)
                        * (1.0 - f64::from(second_distance)).max(0.0))
                    .sqrt()
                    * middle
                    * conserved(source_volume, merged_volume)
                    * separated(
                        first_position.distance(second_position),
                        detections.radius(first_point),
                        detections.radius(second_point),
                    )
                    * evidence;
                events.push(Event::Join(Join {
                    frame: time,
                    head: first_end,
                    other: second_end,
                    child: Some(child),
                    score,
                }));
            }
        }
    }
}

fn start_chains(chains: &Chains, frame: usize) -> (HashMap<usize, usize>, Vec<usize>) {
    let time = frame as u32;
    let mut starts = HashMap::new();
    let mut order = Vec::new();
    for index in 0..chains.len() {
        let chain = chains.chain(index);
        if chain.start() == time && chain.incoming() == 0 {
            starts.insert(chain.points()[0], index);
            order.push(index);
        }
    }
    (starts, order)
}

fn end_chains(chains: &Chains, frame: usize) -> HashMap<usize, usize> {
    let time = (frame - 1) as u32;
    let mut ends = HashMap::new();
    for index in 0..chains.len() {
        let chain = chains.chain(index);
        if chain.end() == time && chain.outgoing() == 0 {
            ends.insert(*chain.points().last().expect("a chain holds points"), index);
        }
    }
    ends
}

fn predict(
    detections: &Detections,
    chains: &Chains,
    chain: usize,
    position: usize,
    frame: u32,
) -> Vec3 {
    let points = chains.points(chain);
    let at = points[position];
    let current = detections.centroid(at);
    if position == 0 {
        return current;
    }
    let previous = points[position - 1];
    let elapsed = (detections.frame(at) - detections.frame(previous)) as f32;
    let velocity = (current - detections.centroid(previous)) / elapsed;
    current + velocity * (frame - detections.frame(at)) as f32
}

fn overlap_of(detections: &Detections, first: usize, second: usize) -> f32 {
    let reach = detections.radius(first) + detections.radius(second);
    (reach
        - detections
            .centroid(first)
            .distance(detections.centroid(second)))
        / reach
}

fn select(events: &[Event]) -> Vec<usize> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut owners: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        for chain in event.chains() {
            owners.entry(chain).or_default().push(index);
        }
    }
    let mut adjacency = vec![Vec::new(); events.len()];
    for members in owners.values() {
        for &first in members {
            for &second in members {
                if first != second && !adjacency[first].contains(&second) {
                    adjacency[first].push(second);
                }
            }
        }
    }
    let mut roots = (0..events.len()).collect::<Vec<_>>();
    for (first, neighbours) in adjacency.iter().enumerate() {
        for &second in neighbours {
            let first_root = root(&mut roots, first);
            let second_root = root(&mut roots, second);
            if first_root != second_root {
                roots[second_root] = first_root;
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for index in 0..events.len() {
        let found = root(&mut roots, index);
        groups.entry(found).or_default().push(index);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_unstable_by_key(|group| group[0]);
    let mut chosen = Vec::new();
    for members in groups {
        chosen.append(&mut component(events, &adjacency, &members));
    }
    chosen.sort_unstable();
    chosen
}

fn root(roots: &mut [usize], index: usize) -> usize {
    let mut current = index;
    while roots[current] != current {
        current = roots[current];
    }
    let mut walk = index;
    while roots[walk] != current {
        let next = roots[walk];
        roots[walk] = current;
        walk = next;
    }
    current
}

fn component(events: &[Event], adjacency: &[Vec<usize>], members: &[usize]) -> Vec<usize> {
    if members.len() == 1 {
        return vec![members[0]];
    }
    let slot_of = members
        .iter()
        .enumerate()
        .map(|(position, &global)| (global, position))
        .collect::<HashMap<_, _>>();
    let mut neighbours = vec![Vec::new(); members.len()];
    for (position, &global) in members.iter().enumerate() {
        for &other in &adjacency[global] {
            if let Some(&other) = slot_of.get(&other) {
                neighbours[position].push(other);
            }
        }
    }
    let mut order = (0..members.len()).collect::<Vec<_>>();
    order.sort_unstable_by(|first, second| {
        events[members[*second]]
            .score()
            .total_cmp(&events[members[*first]].score())
            .then_with(|| first.cmp(second))
    });
    let scores = order
        .iter()
        .map(|&local| events[members[local]].score())
        .collect::<Vec<_>>();
    let mut slot = vec![0usize; members.len()];
    for (position, &local) in order.iter().enumerate() {
        slot[local] = position;
    }
    let neighbours = order
        .iter()
        .map(|&local| {
            neighbours[local]
                .iter()
                .map(|&other| slot[other])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut search = Search {
        scores: &scores,
        neighbours: &neighbours,
        best: 0.0,
        picked: Vec::new(),
        chosen: Vec::new(),
    };
    search.branch(&(0..members.len()).collect::<Vec<_>>(), 0.0);
    search
        .chosen
        .into_iter()
        .map(|position| members[order[position]])
        .collect()
}

struct Search<'a> {
    scores: &'a [f64],
    neighbours: &'a [Vec<usize>],
    best: f64,
    picked: Vec<usize>,
    chosen: Vec<usize>,
}

impl Search<'_> {
    fn branch(&mut self, candidates: &[usize], score: f64) {
        if candidates.is_empty() {
            if score > self.best + EPSILON {
                self.best = score;
                self.chosen.clone_from(&self.picked);
            }
            return;
        }
        let remaining = candidates
            .iter()
            .map(|&candidate| self.scores[candidate])
            .sum::<f64>();
        if score + remaining <= self.best + EPSILON {
            return;
        }
        let first = candidates[0];
        self.picked.push(first);
        let rest = candidates[1..]
            .iter()
            .copied()
            .filter(|candidate| !self.neighbours[first].contains(candidate))
            .collect::<Vec<_>>();
        self.branch(&rest, score + self.scores[first]);
        self.picked.pop();
        self.branch(&candidates[1..], score);
    }
}
