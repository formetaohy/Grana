use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3, Vec3};

const RING_LIMIT: usize = 1 << 12;
const AREA_FLOOR: f64 = 1.0e-14;

pub(super) struct Cap {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) triangles: Vec<u32>,
    pub(super) area: f64,
}

struct Boundary {
    points: Vec<[f32; 3]>,
    edges: Vec<(u32, u32)>,
    joined: HashSet<(u32, u32)>,
}

impl Cap {
    fn empty() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            triangles: Vec::new(),
            area: 0.0,
        }
    }

    fn ring(&mut self, boundary: &Boundary, cycle: &[u32]) {
        let ring = cycle.iter().rev().copied().collect::<Vec<u32>>();
        let points = ring
            .iter()
            .map(|vertex| DVec3::from(boundary.points[*vertex as usize].map(f64::from)))
            .collect::<Vec<_>>();
        let normal = newell(&points).normalize_or_zero();
        if normal == DVec3::ZERO {
            return;
        }
        let normal = normal.as_vec3();
        match triangulate(&points, normal, &boundary.joined) {
            Some(faces) => {
                let slots = self.slots(boundary, &ring, normal);
                for face in faces {
                    let corners = face.map(|index| slots[index]);
                    if corners[0] == corners[1]
                        || corners[1] == corners[2]
                        || corners[2] == corners[0]
                    {
                        continue;
                    }
                    self.area += triangle_area(&points, face);
                    self.triangles.extend_from_slice(&corners);
                }
            }
            None => {
                let pairs = (0..ring.len())
                    .map(|position| (ring[position], ring[(position + 1) % ring.len()]))
                    .collect::<Vec<_>>();
                self.spread(boundary, &pairs, normal);
            }
        }
    }

    fn face(&mut self, boundary: &Boundary, group: &[usize]) {
        let mut members = group
            .iter()
            .map(|slot| boundary.edges[*slot])
            .collect::<Vec<_>>();
        members.sort_unstable();
        let mut normal = DVec3::ZERO;
        for (from, to) in &members {
            let first = DVec3::from(boundary.points[*from as usize].map(f64::from));
            let second = DVec3::from(boundary.points[*to as usize].map(f64::from));
            normal -= first.cross(second);
        }
        let normal = normal.normalize_or_zero();
        if normal == DVec3::ZERO {
            return;
        }
        let pairs = members
            .iter()
            .map(|(from, to)| (*to, *from))
            .collect::<Vec<_>>();
        self.spread(boundary, &pairs, normal.as_vec3());
    }

    fn spread(&mut self, boundary: &Boundary, pairs: &[(u32, u32)], normal: Vec3) {
        let mut slots: HashMap<u32, u32> = HashMap::new();
        let mut vertices: Vec<u32> = Vec::new();
        let mut centre = DVec3::ZERO;
        let mut mapped: Vec<(u32, u32)> = Vec::with_capacity(pairs.len());
        for (first, second) in pairs {
            let mut ends = [0u32; 2];
            for (slot, vertex) in ends.iter_mut().zip([*first, *second]) {
                *slot = *slots.entry(vertex).or_insert_with(|| {
                    let index = vertices.len() as u32;
                    vertices.push(vertex);
                    centre += DVec3::from(boundary.points[vertex as usize].map(f64::from));
                    index
                });
            }
            mapped.push((ends[0], ends[1]));
        }
        if vertices.is_empty() {
            return;
        }
        let base = self.positions.len() as u32;
        self.positions
            .push((centre / vertices.len() as f64).as_vec3().to_array());
        self.normals.push(normal.to_array());
        for vertex in &vertices {
            self.positions.push(boundary.points[*vertex as usize]);
            self.normals.push(normal.to_array());
        }
        for (first, second) in mapped {
            let corners = [base, base + 1 + first, base + 1 + second];
            if corners[1] == corners[2] {
                continue;
            }
            let [first, second, third] =
                corners.map(|index| DVec3::from(self.positions[index as usize].map(f64::from)));
            let area = 0.5 * (second - first).cross(third - first).length();
            if area.is_finite() && area > 0.0 {
                self.area += area;
            }
            self.triangles.extend_from_slice(&corners);
        }
    }

    fn slots(&mut self, boundary: &Boundary, ring: &[u32], normal: Vec3) -> Vec<u32> {
        let mut slots = Vec::with_capacity(ring.len());
        let mut seen: HashMap<u32, u32> = HashMap::with_capacity(ring.len());
        for vertex in ring {
            slots.push(*seen.entry(*vertex).or_insert_with(|| {
                let slot = self.positions.len() as u32;
                self.positions.push(boundary.points[*vertex as usize]);
                self.normals.push(normal.to_array());
                slot
            }));
        }
        slots
    }
}

pub(super) fn build(positions: &[[f32; 3]], triangles: &[[u32; 3]], selected: &[usize]) -> Cap {
    let boundary = collect(positions, triangles, selected);
    let mut cap = Cap::empty();
    if boundary.edges.is_empty() {
        return cap;
    }
    for group in groups(&boundary) {
        if simple(&boundary.edges, &group) {
            for cycle in cycles(&boundary.edges, &group) {
                cap.ring(&boundary, &cycle);
            }
        } else {
            cap.face(&boundary, &group);
        }
    }
    cap
}

fn collect(positions: &[[f32; 3]], triangles: &[[u32; 3]], selected: &[usize]) -> Boundary {
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut index: HashMap<[u32; 3], u32> = HashMap::new();
    let mut directed: HashMap<(u32, u32), u32> = HashMap::with_capacity(selected.len() * 3);
    let mut counted: HashMap<(u32, u32), u32> = HashMap::with_capacity(selected.len() * 3);
    for slot in selected {
        let triangle = triangles[*slot];
        let mut mapped = [0u32; 3];
        for (corner, vertex) in triangle.iter().enumerate() {
            let point = positions[*vertex as usize];
            mapped[corner] = *index.entry(point.map(f32::to_bits)).or_insert_with(|| {
                points.push(point);
                points.len() as u32 - 1
            });
        }
        for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
            let (from, to) = (mapped[pair[0]], mapped[pair[1]]);
            *directed.entry((from, to)).or_insert(0) += 1;
            *counted.entry((from.min(to), from.max(to))).or_insert(0) += 1;
        }
    }
    let joined = counted
        .iter()
        .filter(|(_, count)| **count >= 2)
        .map(|(edge, _)| *edge)
        .collect::<HashSet<_>>();
    let mut edges = directed
        .iter()
        .filter(|((from, to), count)| **count == 1 && !directed.contains_key(&(*to, *from)))
        .map(|(edge, _)| *edge)
        .collect::<Vec<_>>();
    edges.sort_unstable();
    Boundary {
        points,
        edges,
        joined,
    }
}

fn groups(boundary: &Boundary) -> Vec<Vec<usize>> {
    let mut parent = (0..boundary.points.len() as u32).collect::<Vec<_>>();
    for (from, to) in &boundary.edges {
        union(&mut parent, *from, *to);
    }
    let mut roots: HashMap<u32, usize> = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (position, (from, _)) in boundary.edges.iter().enumerate() {
        let root = root(&mut parent, *from);
        let vacant = groups.len();
        let index = *roots.entry(root).or_insert(vacant);
        if index == vacant {
            groups.push(Vec::new());
        }
        groups[index].push(position);
    }
    groups
}

fn simple(edges: &[(u32, u32)], group: &[usize]) -> bool {
    let mut sources = HashSet::new();
    let mut targets = HashSet::new();
    group.iter().all(|slot| {
        let (from, to) = edges[*slot];
        sources.insert(from) && targets.insert(to)
    })
}

fn cycles(edges: &[(u32, u32)], group: &[usize]) -> Vec<Vec<u32>> {
    let mut following: HashMap<u32, u32> = HashMap::with_capacity(group.len());
    for slot in group {
        let (from, to) = edges[*slot];
        following.insert(from, to);
    }
    let mut starts = following.keys().copied().collect::<Vec<_>>();
    starts.sort_unstable();
    let mut cycles = Vec::new();
    for start in starts {
        if !following.contains_key(&start) {
            continue;
        }
        let mut cycle = vec![start];
        let mut current = start;
        while let Some(next) = following.remove(&current) {
            if next == start {
                break;
            }
            cycle.push(next);
            current = next;
        }
        if cycle.len() >= 3 {
            cycles.push(cycle);
        }
    }
    cycles
}

fn union(parent: &mut [u32], left: u32, right: u32) {
    let left = root(parent, left);
    let right = root(parent, right);
    if left == right {
        return;
    }
    let (low, high) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    parent[high as usize] = low;
}

fn root(parent: &mut [u32], vertex: u32) -> u32 {
    let mut found = vertex;
    while parent[found as usize] != found {
        found = parent[found as usize];
    }
    let mut current = vertex;
    while parent[current as usize] != found {
        let next = parent[current as usize];
        parent[current as usize] = found;
        current = next;
    }
    found
}

fn newell(points: &[DVec3]) -> DVec3 {
    let mut normal = DVec3::ZERO;
    for (position, point) in points.iter().enumerate() {
        let next = points[(position + 1) % points.len()];
        normal += point.cross(next);
    }
    normal
}

fn dominant(normal: Vec3) -> usize {
    let mut axis = 0;
    for slot in 1..3 {
        if normal[slot].abs() > normal[axis].abs() {
            axis = slot;
        }
    }
    axis
}

fn key(first: usize, second: usize) -> (u32, u32) {
    if first < second {
        (first as u32, second as u32)
    } else {
        (second as u32, first as u32)
    }
}

fn triangulate(
    points: &[DVec3],
    normal: Vec3,
    joined: &HashSet<(u32, u32)>,
) -> Option<Vec<[usize; 3]>> {
    if points.len() > RING_LIMIT {
        return None;
    }
    let centre = points.iter().copied().sum::<DVec3>() / points.len() as f64;
    let reach = points
        .iter()
        .map(|point| (*point - centre).length())
        .fold(0.0, f64::max);
    let lift = points
        .iter()
        .map(|point| (*point - centre).dot(DVec3::from(normal)))
        .fold(0.0f64, |worst, value| worst.max(value.abs()));
    if reach <= 0.0 || lift > 0.1 * reach {
        return None;
    }
    let axis = dominant(normal);
    let (across, down) = ((axis + 1) % 3, (axis + 2) % 3);
    let flip = if normal[axis] < 0.0 { -1.0 } else { 1.0 };
    let flat = points
        .iter()
        .map(|point| DVec2::new(point[across], flip * point[down]))
        .collect::<Vec<_>>();
    if signed_area(&flat) <= AREA_FLOOR {
        return None;
    }
    clip(&flat, joined)
}

fn signed_area(points: &[DVec2]) -> f64 {
    let mut area = 0.0;
    for (position, point) in points.iter().enumerate() {
        let next = points[(position + 1) % points.len()];
        area += point.perp_dot(next);
    }
    0.5 * area
}

fn clip(points: &[DVec2], joined: &HashSet<(u32, u32)>) -> Option<Vec<[usize; 3]>> {
    let count = points.len();
    let mut order = (0..count).collect::<Vec<usize>>();
    let mut faces = Vec::with_capacity(count.saturating_sub(2));
    while order.len() > 3 {
        let length = order.len();
        let mut clipped = false;
        for position in 0..length {
            let previous = order[(position + length - 1) % length];
            let current = order[position];
            let next = order[(position + 1) % length];
            if (points[current] - points[previous]).perp_dot(points[next] - points[current])
                <= AREA_FLOOR
            {
                continue;
            }
            if joined.contains(&key(previous, next)) {
                continue;
            }
            if occludes(points, &order, previous, current, next) {
                continue;
            }
            faces.push([previous, current, next]);
            order.remove(position);
            clipped = true;
            break;
        }
        if !clipped {
            return None;
        }
    }
    if joined.contains(&key(order[0], order[2])) {
        return None;
    }
    faces.push([order[0], order[1], order[2]]);
    Some(faces)
}

fn occludes(points: &[DVec2], order: &[usize], first: usize, second: usize, third: usize) -> bool {
    let [left, middle, right] = [points[first], points[second], points[third]];
    let double = (middle - left).perp_dot(right - left);
    if double.abs() <= AREA_FLOOR {
        return false;
    }
    order.iter().any(|slot| {
        if *slot == first || *slot == second || *slot == third {
            return false;
        }
        let point = points[*slot] - left;
        let across = (middle - left).perp_dot(point) / double;
        let down = (right - left).perp_dot(point) / double;
        across > 0.0 && down > 0.0 && across + down < 1.0
    })
}

fn triangle_area(points: &[DVec3], face: [usize; 3]) -> f64 {
    let [first, second, third] = face.map(|index| points[index]);
    let area = 0.5 * (second - first).cross(third - first).length();
    if area.is_finite() && area > 0.0 {
        area
    } else {
        0.0
    }
}
