use glam::Vec3;
use grana_object::{Track, TrackNode, TrackSet};

use crate::assignment::{self, rejected};
use grana_object::TrackParams;

struct Segment {
    nodes: Vec<TrackNode>,
    parent: Option<usize>,
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

pub struct TrackBuilder {
    params: TrackParams,
    segments: Vec<Segment>,
    time: Option<u32>,
}

impl TrackBuilder {
    pub fn new(params: TrackParams) -> Self {
        Self {
            params,
            segments: Vec::new(),
            time: None,
        }
    }

    pub fn push(&mut self, time: u32, centroids: &[[f32; 3]]) {
        if let Some(previous) = self.time {
            assert!(
                time > previous,
                "tracked frames arrive in increasing time order"
            );
        }
        self.time = Some(time);
        self.close_expired(time);
        let active = self.active(time);
        if active.is_empty() && centroids.is_empty() {
            return;
        }
        let mut matches = self.matches(&active, time, centroids);
        let mut used = vec![false; centroids.len()];
        for detection in matches.iter().flatten() {
            used[*detection] = true;
        }
        let (divisions, consumed) = self.divisions(&active, time, centroids, &matches, &used);
        for division in &divisions {
            used[division.first] = true;
            used[division.second] = true;
        }
        for (consumed, detection) in consumed.iter().zip(matches.iter_mut()) {
            if *consumed {
                *detection = None;
            }
        }
        for division in divisions {
            self.segments[division.parent].closed = true;
            self.push_segment(
                Some(division.parent),
                time,
                division.first,
                centroids[division.first],
            );
            self.push_segment(
                Some(division.parent),
                time,
                division.second,
                centroids[division.second],
            );
        }
        for (row, &index) in active.iter().enumerate() {
            let Some(detection) = matches[row] else {
                continue;
            };
            self.segments[index]
                .nodes
                .push(TrackNode::new(time, detection, centroids[detection]));
        }
        for (detection, used) in used.into_iter().enumerate() {
            if !used {
                self.push_segment(None, time, detection, centroids[detection]);
            }
        }
    }

    pub fn finish(self, frame_interval: f32) -> TrackSet {
        let tracks = self
            .segments
            .into_iter()
            .map(|segment| Track::new(segment.parent.map(|parent| parent as u32), segment.nodes))
            .collect();
        TrackSet::new(frame_interval, tracks)
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

    fn matches(&self, active: &[usize], time: u32, centroids: &[[f32; 3]]) -> Vec<Option<usize>> {
        let detections = centroids.len();
        let tracks = active.len();
        if tracks == 0 || detections == 0 {
            return vec![None; tracks];
        }
        let mut cost = vec![vec![rejected(); detections + tracks]; tracks];
        for (row, &index) in active.iter().enumerate() {
            let segment = &self.segments[index];
            let gap = f64::from(time - segment.last_time());
            let allowed = f64::from(self.params.max_step()) * gap;
            let predicted = segment.predict(time);
            cost[row][detections + row] = 0.0;
            for (column, centroid) in centroids.iter().enumerate() {
                let distance = predicted.distance(Vec3::from(*centroid)) as f64;
                if distance <= allowed {
                    cost[row][column] = distance / allowed - 1.0;
                }
            }
        }
        let assignment = assignment::assign(&cost, detections + tracks);
        let mut matches = vec![None; tracks];
        for (row, column) in assignment.into_iter().enumerate() {
            if column < detections {
                matches[row] = Some(column);
            }
        }
        matches
    }

    fn divisions(
        &self,
        active: &[usize],
        time: u32,
        centroids: &[[f32; 3]],
        matches: &[Option<usize>],
        used: &[bool],
    ) -> (Vec<Division>, Vec<bool>) {
        let mut divisions = Vec::new();
        let mut consumed = vec![false; active.len()];
        if !self.params.division() {
            return (divisions, consumed);
        }
        let positions = centroids
            .iter()
            .map(|centroid| Vec3::from(*centroid))
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

    fn push_segment(
        &mut self,
        parent: Option<usize>,
        time: u32,
        object: usize,
        centroid: [f32; 3],
    ) {
        self.segments.push(Segment {
            nodes: vec![TrackNode::new(time, object, centroid)],
            parent,
            closed: false,
        });
    }
}
