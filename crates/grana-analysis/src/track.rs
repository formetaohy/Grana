use glam::Vec3;
use grana_object::{Link, Track, TrackDetection, TrackLink, TrackNode, TrackParams, TrackSet};

use crate::assignment::{self, rejected};

struct Segment {
    nodes: Vec<TrackNode>,
    radius: f32,
    closed: bool,
}

impl Segment {
    fn last_time(&self) -> u32 {
        self.nodes.last().expect("a track holds nodes").time()
    }

    fn predict(&self, time: u32) -> Vec3 {
        let last = self.nodes.last().expect("a track holds nodes");
        if self.nodes.len() < 2 {
            return Vec3::from(last.centroid());
        }
        let previous = self.nodes[self.nodes.len() - 2];
        let elapsed = (last.time() - previous.time()) as f32;
        let velocity = (Vec3::from(last.centroid()) - Vec3::from(previous.centroid())) / elapsed;
        Vec3::from(last.centroid()) + velocity * (time - last.time()) as f32
    }
}

struct Division {
    parent: usize,
    first: usize,
    second: usize,
}

struct Merge {
    parent: usize,
    other: usize,
    detection: usize,
}

pub struct TrackBuilder {
    params: TrackParams,
    segments: Vec<Segment>,
    links: Vec<(usize, usize, Link)>,
    time: Option<u32>,
}

impl TrackBuilder {
    pub fn new(params: TrackParams) -> Self {
        Self {
            params,
            segments: Vec::new(),
            links: Vec::new(),
            time: None,
        }
    }

    pub fn push(&mut self, time: u32, detections: &[TrackDetection]) {
        if let Some(previous) = self.time {
            assert!(
                time > previous,
                "tracked frames arrive in increasing time order"
            );
        }
        self.time = Some(time);
        self.close_expired(time);
        let active = self.active(time);
        if active.is_empty() && detections.is_empty() {
            return;
        }
        let mut matches = self.matches(&active, time, detections);
        let mut used = vec![false; detections.len()];
        for detection in matches.iter().flatten() {
            used[*detection] = true;
        }
        let (divisions, consumed) = self.divisions(&active, time, detections, &matches, &used);
        for division in &divisions {
            used[division.first] = true;
            used[division.second] = true;
        }
        let (merges, merged) = self.merges(&active, time, detections, &matches, &consumed);
        for (consumed, detection) in consumed.iter().zip(matches.iter_mut()) {
            if *consumed {
                *detection = None;
            }
        }
        for division in divisions {
            self.segments[division.parent].closed = true;
            self.push_segment(
                Some((division.parent, Link::Division)),
                time,
                division.first,
                detections[division.first],
            );
            self.push_segment(
                Some((division.parent, Link::Division)),
                time,
                division.second,
                detections[division.second],
            );
        }
        for merge in &merges {
            self.segments[active[merge.parent]].closed = true;
            self.segments[active[merge.other]].closed = true;
        }
        for merge in merges {
            self.push_merged_segment(
                active[merge.parent],
                active[merge.other],
                time,
                merge.detection,
                detections[merge.detection],
            );
        }
        for (row, &index) in active.iter().enumerate() {
            if merged[row] {
                continue;
            }
            let Some(matched) = matches[row] else {
                continue;
            };
            let detection = detections[matched];
            if self.segments[index].last_time() + 1 == time {
                self.segments[index].radius = detection.radius();
                self.segments[index].nodes.push(TrackNode::new(
                    time,
                    matched,
                    detection.centroid(),
                ));
            } else {
                self.segments[index].closed = true;
                self.push_segment(Some((index, Link::Gap)), time, matched, detection);
            }
        }
        for (detection, used) in used.into_iter().enumerate() {
            if !used {
                self.push_segment(None, time, detection, detections[detection]);
            }
        }
    }

    pub fn finish(self, frame_interval: f32) -> TrackSet {
        let tracks = self
            .segments
            .into_iter()
            .map(|segment| Track::new(segment.nodes))
            .collect::<Vec<_>>();
        let links = self
            .links
            .into_iter()
            .map(|(parent, child, kind)| TrackLink::new(parent as u32, child as u32, kind))
            .collect::<Vec<_>>();
        TrackSet::with_links(frame_interval, tracks, links)
    }

    fn close_expired(&mut self, time: u32) {
        let limit = u64::from(time);
        for segment in &mut self.segments {
            if segment.closed {
                continue;
            }
            let reachable = u64::from(segment.last_time()) + 1 + u64::from(self.params.max_gap());
            if reachable < limit {
                segment.closed = true;
            }
        }
    }

    fn active(&self, time: u32) -> Vec<usize> {
        self.segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| !segment.closed && segment.last_time() < time)
            .map(|(index, _)| index)
            .collect()
    }

    fn matches(
        &self,
        active: &[usize],
        time: u32,
        detections: &[TrackDetection],
    ) -> Vec<Option<usize>> {
        let count = detections.len();
        let tracks = active.len();
        if tracks == 0 || count == 0 {
            return vec![None; tracks];
        }
        let mut cost = vec![vec![rejected(); count + tracks]; tracks];
        for (row, &index) in active.iter().enumerate() {
            let segment = &self.segments[index];
            let gap = f64::from(time - segment.last_time());
            let allowed = f64::from(self.params.max_step()) * gap;
            let predicted = segment.predict(time);
            cost[row][count + row] = 0.0;
            for (column, detection) in detections.iter().enumerate() {
                let distance = predicted.distance(Vec3::from(detection.centroid())) as f64;
                if distance <= allowed {
                    cost[row][column] = distance / allowed - 1.0;
                }
            }
        }
        let assignment = assignment::assign(&cost, count + tracks);
        let mut matches = vec![None; tracks];
        for (row, column) in assignment.into_iter().enumerate() {
            if column < count {
                matches[row] = Some(column);
            }
        }
        matches
    }

    fn divisions(
        &self,
        active: &[usize],
        time: u32,
        detections: &[TrackDetection],
        matches: &[Option<usize>],
        used: &[bool],
    ) -> (Vec<Division>, Vec<bool>) {
        let mut divisions = Vec::new();
        let mut consumed = vec![false; active.len()];
        if !self.params.division() {
            return (divisions, consumed);
        }
        let positions = detections
            .iter()
            .map(|detection| Vec3::from(detection.centroid()))
            .collect::<Vec<_>>();
        let range = self.params.division_range();
        let mut taken = used.to_vec();
        for (row, &parent) in active.iter().enumerate() {
            let segment = &self.segments[parent];
            if segment.last_time() + 1 != time {
                continue;
            }
            let Some(first) = matches[row] else {
                continue;
            };
            let predicted = segment.predict(time);
            if predicted.distance(positions[first]) > range {
                continue;
            }
            let mut best: Option<(f32, usize)> = None;
            for (second, position) in positions.iter().enumerate() {
                if taken[second] || second == first {
                    continue;
                }
                let span = predicted.distance(*position);
                let separation = positions[first].distance(*position);
                if span > range || separation > range {
                    continue;
                }
                let cost = predicted.distance(positions[first]) + span + separation;
                if best.is_none_or(|(nearest, _)| cost < nearest) {
                    best = Some((cost, second));
                }
            }
            if let Some((_, second)) = best {
                taken[second] = true;
                divisions.push(Division {
                    parent,
                    first,
                    second,
                });
                consumed[row] = true;
            }
        }
        for (row, &parent) in active.iter().enumerate() {
            if matches[row].is_some() {
                continue;
            }
            let segment = &self.segments[parent];
            if segment.last_time() + 1 != time {
                continue;
            }
            let predicted = segment.predict(time);
            let mut best: Option<(f32, usize, usize)> = None;
            for first in 0..positions.len() {
                if taken[first] {
                    continue;
                }
                let first_span = predicted.distance(positions[first]);
                if first_span > range {
                    continue;
                }
                for second in first + 1..positions.len() {
                    if taken[second] {
                        continue;
                    }
                    let second_span = predicted.distance(positions[second]);
                    let separation = positions[first].distance(positions[second]);
                    if second_span > range || separation > range {
                        continue;
                    }
                    let cost = first_span + second_span + separation;
                    if best.is_none_or(|(nearest, _, _)| cost < nearest) {
                        best = Some((cost, first, second));
                    }
                }
            }
            if let Some((_, first, second)) = best {
                taken[first] = true;
                taken[second] = true;
                divisions.push(Division {
                    parent,
                    first,
                    second,
                });
                consumed[row] = true;
            }
        }
        (divisions, consumed)
    }

    fn merges(
        &self,
        active: &[usize],
        time: u32,
        detections: &[TrackDetection],
        matches: &[Option<usize>],
        divided: &[bool],
    ) -> (Vec<Merge>, Vec<bool>) {
        let mut merges = Vec::new();
        let mut consumed = vec![false; active.len()];
        let events = self.params.events();
        if !events.merge() {
            return (merges, consumed);
        }
        let range = events.merge_range();
        let cell = |position: Vec3| {
            (
                (position.x / range).floor() as i64,
                (position.y / range).floor() as i64,
                (position.z / range).floor() as i64,
            )
        };
        let mut grid = std::collections::HashMap::<(i64, i64, i64), Vec<(usize, usize)>>::new();
        for (other, &index) in active.iter().enumerate() {
            let Some(detection) = matches[other] else {
                continue;
            };
            if divided[other] || self.segments[index].last_time() + 1 != time {
                continue;
            }
            grid.entry(cell(Vec3::from(detections[detection].centroid())))
                .or_default()
                .push((other, detection));
        }
        for (row, &parent) in active.iter().enumerate() {
            if matches[row].is_some() || divided[row] || consumed[row] {
                continue;
            }
            let segment = &self.segments[parent];
            if segment.last_time() + 1 != time {
                continue;
            }
            let predicted = segment.predict(time);
            let (x, y, z) = cell(predicted);
            let mut best: Option<(f32, usize, usize)> = None;
            for offset in 0..27 {
                let dx = offset % 3 - 1;
                let dy = offset / 3 % 3 - 1;
                let dz = offset / 9 - 1;
                let Some(candidates) = grid.get(&(x + dx as i64, y + dy as i64, z + dz as i64))
                else {
                    continue;
                };
                for &(other, detection) in candidates {
                    if consumed[other] {
                        continue;
                    }
                    let merged = Vec3::from(detections[detection].centroid());
                    let other_segment = &self.segments[active[other]];
                    let other_predicted = other_segment.predict(time);
                    let first = predicted.distance(merged);
                    let second = other_predicted.distance(merged);
                    if first > range || second > range {
                        continue;
                    }
                    let reach = segment.radius + other_segment.radius;
                    let overlap = (reach - predicted.distance(other_predicted)) / reach;
                    if overlap < events.merge_overlap() {
                        continue;
                    }
                    let cost = first + second;
                    if best.is_none_or(|(nearest, _, _)| cost < nearest) {
                        best = Some((cost, other, detection));
                    }
                }
            }
            if let Some((_, other, detection)) = best {
                consumed[row] = true;
                consumed[other] = true;
                merges.push(Merge {
                    parent: row,
                    other,
                    detection,
                });
            }
        }
        (merges, consumed)
    }

    fn push_segment(
        &mut self,
        parent: Option<(usize, Link)>,
        time: u32,
        object: usize,
        detection: TrackDetection,
    ) {
        let index = self.segments.len();
        if let Some((parent, kind)) = parent {
            self.links.push((parent, index, kind));
        }
        self.segments.push(Segment {
            nodes: vec![TrackNode::new(time, object, detection.centroid())],
            radius: detection.radius(),
            closed: false,
        });
    }

    fn push_merged_segment(
        &mut self,
        first: usize,
        second: usize,
        time: u32,
        object: usize,
        detection: TrackDetection,
    ) {
        assert!(first != second, "a merge joins two distinct tracks");
        let index = self.segments.len();
        self.links.push((first, index, Link::Merge));
        self.links.push((second, index, Link::Merge));
        self.segments.push(Segment {
            nodes: vec![TrackNode::new(time, object, detection.centroid())],
            radius: detection.radius(),
            closed: false,
        });
    }
}
