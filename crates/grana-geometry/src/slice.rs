use std::collections::HashMap;

use glam::Vec3;

use crate::PackedMesh;

const UNIT: f32 = 65535.0;
const WIDE_SPAN: usize = 8;
const BIN_FLOOR: usize = 16;
const BIN_CEIL: usize = 256;
const BIN_TRIANGLES: usize = 16;
const BAND_LIMIT: usize = 1 << 12;
pub const SECTION_QUADS: usize = 1 << 19;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPlane {
    pub normal: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub offset: f32,
}

impl SectionPlane {
    pub fn new(normal: Vec3, right: Vec3, offset: f32) -> Self {
        assert!(
            normal.is_finite() && right.is_finite() && offset.is_finite(),
            "a section plane is a finite world plane"
        );
        let normal = normal.normalize();
        let right = (right - normal * right.dot(normal)).normalize();
        Self {
            normal: normal.to_array(),
            right: right.to_array(),
            up: normal.cross(right).to_array(),
            offset,
        }
    }

    pub fn distance(&self, point: [f32; 3]) -> f32 {
        let normal = Vec3::from(self.normal);
        normal.dot(Vec3::from(point)) - self.offset
    }

    pub fn project(&self, point: [f32; 3]) -> [f32; 2] {
        let point = Vec3::from(point);
        [
            Vec3::from(self.right).dot(point),
            Vec3::from(self.up).dot(point),
        ]
    }

    fn projection_range(&self, low: [f32; 3], scale: [f32; 3]) -> (f32, f32) {
        let mut minimum = 0.0f32;
        let mut maximum = 0.0f32;
        for axis in 0..3 {
            let start = self.normal[axis] * low[axis];
            let end = start + self.normal[axis] * UNIT * scale[axis];
            minimum += start.min(end);
            maximum += start.max(end);
        }
        (minimum, maximum)
    }
}

#[derive(Clone, Copy)]
struct Edge {
    low: f32,
    high: f32,
    across: f32,
    slope: f32,
}

impl Edge {
    fn at(self, up: f32) -> f32 {
        self.across + (up - self.low) * self.slope
    }
}

#[derive(Clone, Copy)]
struct Crossing {
    middle: f32,
    low: f32,
    high: f32,
}

pub struct SectionIndex {
    low: f32,
    inverse: f32,
    bins: usize,
    offsets: Vec<u32>,
    triangles: Vec<u32>,
    wide: Vec<u32>,
}

impl SectionIndex {
    pub fn new(mesh: &PackedMesh, object: usize, normal: [f32; 3]) -> Self {
        let plane = SectionPlane::new(Vec3::from(normal), perpendicular(normal), 0.0);
        let group = &mesh.groups()[object];
        let (low, high) = plane.projection_range(group.min(), group.scale());
        let count = group.index_count() as usize / 3;
        let bins = (count / BIN_TRIANGLES).clamp(BIN_FLOOR, BIN_CEIL);
        let span = (high - low).max(1.0e-6);
        let inverse = bins as f32 / span;
        let mut placement = Vec::with_capacity(count);
        let mut counts = vec![0u32; bins];
        let mut wide = Vec::new();
        for triangle in 0..count {
            let corners = mesh.triangle(object, triangle);
            let mut minimum = f32::INFINITY;
            let mut maximum = f32::NEG_INFINITY;
            for corner in corners {
                let projected = plane.distance(corner);
                minimum = minimum.min(projected);
                maximum = maximum.max(projected);
            }
            let first = bin_of(minimum, low, inverse, bins);
            let last = bin_of(maximum, low, inverse, bins);
            if last - first >= WIDE_SPAN {
                wide.push(triangle as u32);
                placement.push((1u32, 0u32));
                continue;
            }
            for count in counts[first..=last].iter_mut() {
                *count += 1;
            }
            placement.push((first as u32, last as u32));
        }
        let mut offsets = vec![0u32; bins + 1];
        for bin in 0..bins {
            offsets[bin + 1] = offsets[bin] + counts[bin];
        }
        let mut triangles = vec![0u32; offsets[bins] as usize];
        let mut cursor = offsets.clone();
        for (triangle, (first, last)) in placement.into_iter().enumerate() {
            for bin in first..=last {
                let slot = &mut cursor[bin as usize];
                triangles[*slot as usize] = triangle as u32;
                *slot += 1;
            }
        }
        Self {
            low,
            inverse,
            bins,
            offsets,
            triangles,
            wide,
        }
    }

    pub fn bytes(&self) -> u64 {
        ((self.offsets.len() + self.triangles.len() + self.wide.len()) * size_of::<u32>()) as u64
    }

    fn bin(&self, plane: f32) -> &[u32] {
        let bin = bin_of(plane, self.low, self.inverse, self.bins);
        let start = self.offsets[bin] as usize;
        let end = self.offsets[bin + 1] as usize;
        &self.triangles[start..end]
    }
}

pub fn perpendicular(normal: [f32; 3]) -> Vec3 {
    let normal = Vec3::from(normal).normalize();
    let guide = if normal.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    normal.cross(guide).normalize()
}

fn bin_of(value: f32, low: f32, inverse: f32, bins: usize) -> usize {
    let position = ((value - low) * inverse).floor();
    position.clamp(0.0, (bins - 1) as f32) as usize
}

pub struct SectionCutter {
    segments: Vec<[[f32; 2]; 2]>,
    edges: Vec<Edge>,
    events: Vec<f32>,
    order: Vec<usize>,
    active: Vec<usize>,
    crossings: Vec<Crossing>,
}

impl SectionCutter {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            edges: Vec::new(),
            events: Vec::new(),
            order: Vec::new(),
            active: Vec::new(),
            crossings: Vec::new(),
        }
    }

    pub fn cut(
        &mut self,
        mesh: &PackedMesh,
        object: usize,
        plane: &SectionPlane,
        vertices: &mut Vec<[f32; 2]>,
    ) -> usize {
        let count = mesh.groups()[object].index_count() as usize / 3;
        self.cut_of(mesh, object, plane, 0..count as u32, vertices)
    }

    pub fn cut_indexed(
        &mut self,
        mesh: &PackedMesh,
        object: usize,
        plane: &SectionPlane,
        index: &SectionIndex,
        vertices: &mut Vec<[f32; 2]>,
    ) -> usize {
        let candidates = index
            .wide
            .iter()
            .copied()
            .chain(index.bin(plane.offset).iter().copied());
        self.cut_of(mesh, object, plane, candidates, vertices)
    }

    fn cut_of(
        &mut self,
        mesh: &PackedMesh,
        object: usize,
        plane: &SectionPlane,
        triangles: impl Iterator<Item = u32>,
        vertices: &mut Vec<[f32; 2]>,
    ) -> usize {
        let group = &mesh.groups()[object];
        let (base, ceiling) = plane.projection_range(group.min(), group.scale());
        if plane.offset < base || plane.offset > ceiling {
            return 0;
        }
        self.segments.clear();
        for triangle in triangles {
            let corners = mesh.triangle(object, triangle as usize);
            let mut points = [[0.0f32; 2]; 3];
            let mut found = 0usize;
            for corner in 0..3 {
                let next = (corner + 1) % 3;
                let from = plane.distance(corners[corner]);
                let to = plane.distance(corners[next]);
                if (from < 0.0) == (to < 0.0) {
                    continue;
                }
                let travel = from / (from - to);
                let mut point = Vec3::from(corners[corner]);
                point += (Vec3::from(corners[next]) - point) * travel;
                points[found] = plane.project(point.to_array());
                found += 1;
            }
            if found != 2 {
                continue;
            }
            self.segments.push([points[0], points[1]]);
        }
        if self.segments.len() < 2 {
            return 0;
        }
        self.edges.clear();
        for pair in &self.segments {
            let mut from = pair[0];
            let mut to = pair[1];
            if from[1] > to[1] {
                std::mem::swap(&mut from, &mut to);
            }
            if from[1] == to[1] {
                continue;
            }
            self.edges.push(Edge {
                low: from[1],
                high: to[1],
                across: from[0],
                slope: (to[0] - from[0]) / (to[1] - from[1]),
            });
        }
        if self.edges.len() < 2 {
            return 0;
        }
        self.events.clear();
        self.events
            .extend(self.edges.iter().flat_map(|edge| [edge.low, edge.high]));
        self.events.sort_by(f32::total_cmp);
        let reach = (ceiling - base).abs().max(1.0);
        self.events
            .dedup_by(|left, right| (*left - *right).abs() <= 1.0e-6 * reach);
        self.quantise();
        self.order.clear();
        self.order.extend(0..self.edges.len());
        self.order
            .sort_by(|left, right| self.edges[*left].low.total_cmp(&self.edges[*right].low));
        let mut pending = 0usize;
        let mut quads = 0usize;
        self.active.clear();
        'bands: for band in self.events.windows(2) {
            let (bottom, top) = (band[0], band[1]);
            if top <= bottom {
                continue;
            }
            while pending < self.order.len() && self.edges[self.order[pending]].low <= bottom {
                self.active.push(self.order[pending]);
                pending += 1;
            }
            self.active.retain(|index| self.edges[*index].high > bottom);
            if self.active.len() < 2 {
                continue;
            }
            self.crossings.clear();
            self.crossings.extend(self.active.iter().map(|index| {
                let edge = self.edges[*index];
                let low = edge.at(bottom);
                let high = edge.at(top);
                Crossing {
                    middle: 0.5 * (low + high),
                    low,
                    high,
                }
            }));
            self.crossings
                .sort_by(|left, right| left.middle.total_cmp(&right.middle));
            for pair in self.crossings.as_chunks::<2>().0 {
                let (left, right) = (pair[0], pair[1]);
                if right.low <= left.low && right.high <= left.high {
                    continue;
                }
                if quads >= SECTION_QUADS {
                    break 'bands;
                }
                vertices.push([left.low, bottom]);
                vertices.push([right.low, bottom]);
                vertices.push([right.high, top]);
                vertices.push([left.high, top]);
                quads += 1;
            }
        }
        quads
    }

    fn quantise(&mut self) {
        if self.events.len() <= BAND_LIMIT {
            return;
        }
        let low = self.events[0];
        let high = self.events[self.events.len() - 1];
        let step = (high - low) / BAND_LIMIT as f32;
        if !(step.is_finite() && step > 0.0) {
            return;
        }
        for event in &mut self.events {
            *event = low + ((*event - low) / step).round() * step;
        }
        self.events.sort_by(f32::total_cmp);
        self.events.dedup_by(|left, right| *left == *right);
    }
}

impl Default for SectionCutter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn is_closed(mesh: &PackedMesh, object: usize) -> bool {
    let group = &mesh.groups()[object];
    let first = group.first_vertex() as usize;
    let base = group.first_index() as usize;
    let wide = group.wide();
    let mut tally: HashMap<(u64, u64), u32> = HashMap::with_capacity(group.index_count() as usize);
    for triangle in 0..group.index_count() as usize / 3 {
        let corners = [0, 1, 2].map(|corner| {
            let index = if wide {
                mesh.wide()[base + triangle * 3 + corner]
            } else {
                u32::from(mesh.narrow()[base + triangle * 3 + corner])
            };
            let position = mesh.vertices()[first + index as usize].position();
            u64::from(position[0]) | u64::from(position[1]) << 16 | u64::from(position[2]) << 32
        });
        for corner in 0..3 {
            let from = corners[corner];
            let to = corners[(corner + 1) % 3];
            if from == to {
                continue;
            }
            let key = if from < to { (from, to) } else { (to, from) };
            *tally.entry(key).or_insert(0u32) += 1;
        }
    }
    tally.values().all(|count| count.is_multiple_of(2))
}
