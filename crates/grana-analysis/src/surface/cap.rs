use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3, Vec3};

const LOOP_LIMIT: usize = 256;
const PLANARITY: f64 = 0.02;
const AREA_FLOOR: f64 = 1.0e-14;
const FOLD_PENALTY: f64 = 4.0;

pub(super) struct Cap {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) triangles: Vec<u32>,
    pub(super) area: f64,
}

struct Boundary {
    edges: Vec<(u32, u32)>,
    occupied: HashSet<(u32, u32)>,
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

    fn ring(&mut self, points: &[[f32; 3]], boundary: &Boundary, cycle: &[u32]) {
        let ring = cycle.iter().rev().copied().collect::<Vec<u32>>();
        if ring.len() < 3 {
            return;
        }
        let normal = newell(points, &ring).normalize_or_zero();
        if normal == DVec3::ZERO {
            return;
        }
        let mut seen = HashSet::with_capacity(ring.len());
        if !ring.iter().all(|vertex| seen.insert(*vertex)) {
            let pairs = pairs_of(&ring);
            self.spread(points, &pairs, normal);
            return;
        }
        let Some(faces) = triangulate(points, &ring, normal, &boundary.occupied) else {
            let pairs = pairs_of(&ring);
            self.spread(points, &pairs, normal);
            return;
        };
        let normal = normal.as_vec3();
        let slots = self.slots(points, &ring, normal);
        for face in faces {
            let corners = face.map(|index| slots[index]);
            if corners[0] == corners[1] || corners[1] == corners[2] || corners[2] == corners[0] {
                continue;
            }
            self.area += self.triangle_area(corners);
            self.triangles.extend_from_slice(&corners);
        }
    }

    fn face(&mut self, points: &[[f32; 3]], boundary: &Boundary, group: &[usize]) {
        let mut members = group
            .iter()
            .map(|slot| boundary.edges[*slot])
            .collect::<Vec<_>>();
        members.sort_unstable();
        let mut normal = DVec3::ZERO;
        for (from, to) in &members {
            let first = DVec3::from(points[*from as usize].map(f64::from));
            let second = DVec3::from(points[*to as usize].map(f64::from));
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
        self.spread(points, &pairs, normal);
    }

    fn spread(&mut self, points: &[[f32; 3]], pairs: &[(u32, u32)], normal: DVec3) {
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
                    centre += DVec3::from(points[vertex as usize].map(f64::from));
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
        self.normals.push(normal.as_vec3().to_array());
        for vertex in &vertices {
            self.positions.push(points[*vertex as usize]);
            self.normals.push(normal.as_vec3().to_array());
        }
        for (first, second) in mapped {
            let corners = [base, base + 1 + first, base + 1 + second];
            if corners[1] == corners[2] {
                continue;
            }
            let area = self.triangle_area(corners);
            if area > 0.0 {
                self.area += area;
            }
            self.triangles.extend_from_slice(&corners);
        }
    }

    fn slots(&mut self, points: &[[f32; 3]], ring: &[u32], normal: Vec3) -> Vec<u32> {
        let mut slots = Vec::with_capacity(ring.len());
        let mut seen: HashMap<u32, u32> = HashMap::with_capacity(ring.len());
        for vertex in ring {
            slots.push(*seen.entry(*vertex).or_insert_with(|| {
                let slot = self.positions.len() as u32;
                self.positions.push(points[*vertex as usize]);
                self.normals.push(normal.to_array());
                slot
            }));
        }
        slots
    }

    fn triangle_area(&self, corners: [u32; 3]) -> f64 {
        let points =
            corners.map(|index| DVec3::from(self.positions[index as usize].map(f64::from)));
        let area = 0.5
            * (points[1] - points[0])
                .cross(points[2] - points[0])
                .length();
        if area.is_finite() && area > 0.0 {
            area
        } else {
            0.0
        }
    }
}

pub(super) fn build(positions: &[[f32; 3]], triangles: &[[u32; 3]], selected: &[usize]) -> Cap {
    let mut cap = Cap::empty();
    if selected.is_empty() {
        return cap;
    }
    let boundary = collect(triangles, selected);
    if boundary.edges.is_empty() {
        return cap;
    }
    for group in groups(&boundary.edges) {
        if simple(&boundary.edges, &group) {
            for cycle in cycles(&boundary.edges, &group) {
                cap.ring(positions, &boundary, &cycle);
            }
        } else {
            cap.face(positions, &boundary, &group);
        }
    }
    cap
}

fn collect(triangles: &[[u32; 3]], selected: &[usize]) -> Boundary {
    let mut directed: HashMap<(u32, u32), u32> = HashMap::with_capacity(selected.len() * 3);
    for slot in selected {
        let triangle = triangles[*slot];
        for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
            *directed
                .entry((triangle[pair[0]], triangle[pair[1]]))
                .or_insert(0) += 1;
        }
    }
    let mut edges = directed
        .iter()
        .filter(|((from, to), count)| **count == 1 && !directed.contains_key(&(*to, *from)))
        .map(|(edge, _)| *edge)
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return Boundary {
            edges,
            occupied: HashSet::new(),
        };
    }
    edges.sort_unstable();
    let mut occupied: HashSet<(u32, u32)> = HashSet::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
            occupied.insert(key(triangle[pair[0]], triangle[pair[1]]));
        }
    }
    Boundary { edges, occupied }
}

fn groups(edges: &[(u32, u32)]) -> Vec<Vec<usize>> {
    let mut parent: HashMap<u32, u32> = HashMap::with_capacity(edges.len() * 2);
    for (from, to) in edges {
        parent.entry(*from).or_insert(*from);
        parent.entry(*to).or_insert(*to);
    }
    for (from, to) in edges {
        let left = find(&mut parent, *from);
        let right = find(&mut parent, *to);
        if left == right {
            continue;
        }
        let (low, high) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        parent.insert(high, low);
    }
    let mut roots: HashMap<u32, usize> = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (position, (from, _)) in edges.iter().enumerate() {
        let root = find(&mut parent, *from);
        let vacant = groups.len();
        let index = *roots.entry(root).or_insert(vacant);
        if index == vacant {
            groups.push(Vec::new());
        }
        groups[index].push(position);
    }
    groups
}

fn find(parent: &mut HashMap<u32, u32>, vertex: u32) -> u32 {
    let mut root = vertex;
    while parent[&root] != root {
        root = parent[&root];
    }
    let mut current = vertex;
    while parent[&current] != root {
        let next = parent[&current];
        parent.insert(current, root);
        current = next;
    }
    root
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
        let mut closed = false;
        while let Some(next) = following.remove(&current) {
            if next == start {
                closed = true;
                break;
            }
            cycle.push(next);
            current = next;
        }
        if closed && cycle.len() >= 3 {
            cycles.push(cycle);
        }
    }
    cycles
}

fn pairs_of(ring: &[u32]) -> Vec<(u32, u32)> {
    (0..ring.len())
        .map(|position| (ring[position], ring[(position + 1) % ring.len()]))
        .collect()
}

fn newell(points: &[[f32; 3]], ring: &[u32]) -> DVec3 {
    let mut normal = DVec3::ZERO;
    for (position, vertex) in ring.iter().enumerate() {
        let next = ring[(position + 1) % ring.len()];
        let first = DVec3::from(points[*vertex as usize].map(f64::from));
        let second = DVec3::from(points[next as usize].map(f64::from));
        normal += first.cross(second);
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

fn key(first: u32, second: u32) -> (u32, u32) {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

fn triangulate(
    points: &[[f32; 3]],
    ring: &[u32],
    normal: DVec3,
    occupied: &HashSet<(u32, u32)>,
) -> Option<Vec<[usize; 3]>> {
    let loop_points = ring
        .iter()
        .map(|vertex| DVec3::from(points[*vertex as usize].map(f64::from)))
        .collect::<Vec<_>>();
    let centre = loop_points.iter().copied().sum::<DVec3>() / loop_points.len() as f64;
    let reach = loop_points
        .iter()
        .map(|point| (*point - centre).length())
        .fold(0.0, f64::max);
    if reach <= 0.0 {
        return None;
    }
    let lift = loop_points
        .iter()
        .map(|point| (*point - centre).dot(normal))
        .fold(0.0f64, |worst, value| worst.max(value.abs()));
    if lift <= PLANARITY * reach || loop_points.len() > LOOP_LIMIT {
        return projected(&loop_points, ring, normal, occupied);
    }
    minimum_area(&loop_points, ring, normal, occupied, false)
        .or_else(|| minimum_area(&loop_points, ring, normal, occupied, true))
        .or_else(|| projected(&loop_points, ring, normal, occupied))
}

fn projected(
    points: &[DVec3],
    ring: &[u32],
    normal: DVec3,
    occupied: &HashSet<(u32, u32)>,
) -> Option<Vec<[usize; 3]>> {
    let count = points.len();
    if count < 3 {
        return None;
    }
    if count == 3 {
        return Some(vec![[0, 1, 2]]);
    }
    let axis = dominant(normal.as_vec3());
    let (across, down) = ((axis + 1) % 3, (axis + 2) % 3);
    let flip = if normal[axis] < 0.0 { -1.0 } else { 1.0 };
    let flat = points
        .iter()
        .map(|point| DVec2::new(point[across], flip * point[down]))
        .collect::<Vec<_>>();
    if signed_area(&flat) <= AREA_FLOOR {
        return None;
    }
    clip(&flat, ring, occupied)
}

fn signed_area(points: &[DVec2]) -> f64 {
    let mut area = 0.0;
    for (position, point) in points.iter().enumerate() {
        let next = points[(position + 1) % points.len()];
        area += point.perp_dot(next);
    }
    0.5 * area
}

fn clip(points: &[DVec2], ring: &[u32], occupied: &HashSet<(u32, u32)>) -> Option<Vec<[usize; 3]>> {
    let count = points.len();
    let mut order = (0..count).collect::<Vec<usize>>();
    let mut faces = Vec::with_capacity(count - 2);
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
            if forbidden(ring, occupied, previous, next) {
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
    if forbidden(ring, occupied, order[0], order[2]) {
        return None;
    }
    faces.push([order[0], order[1], order[2]]);
    Some(faces)
}

fn forbidden(ring: &[u32], occupied: &HashSet<(u32, u32)>, first: usize, second: usize) -> bool {
    let count = ring.len();
    if (first + 1) % count == second || (second + 1) % count == first {
        return false;
    }
    occupied.contains(&key(ring[first], ring[second]))
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

fn minimum_area(
    points: &[DVec3],
    ring: &[u32],
    normal: DVec3,
    occupied: &HashSet<(u32, u32)>,
    folded: bool,
) -> Option<Vec<[usize; 3]>> {
    let count = points.len();
    let mut cost = vec![f64::INFINITY; count * count];
    let mut cut = vec![0usize; count * count];
    for first in 0..count - 1 {
        cost[first * count + first + 1] = 0.0;
    }
    let diagonal = |first: usize, second: usize| -> bool {
        let (low, high) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        if high == low + 1 || (low == 0 && high == count - 1) {
            return true;
        }
        !occupied.contains(&key(ring[low], ring[high]))
    };
    for span in 2..count {
        for first in 0..count - span {
            let last = first + span;
            let mut best = f64::INFINITY;
            let mut choice = 0;
            for middle in first + 1..last {
                let left = cost[first * count + middle];
                let right = cost[middle * count + last];
                if !left.is_finite() || !right.is_finite() {
                    continue;
                }
                if !diagonal(first, middle) || !diagonal(middle, last) {
                    continue;
                }
                let weight = triangle_weight(points, first, middle, last, normal, folded);
                if !weight.is_finite() {
                    continue;
                }
                let total = left + right + weight;
                if total < best {
                    best = total;
                    choice = middle;
                }
            }
            cost[first * count + last] = best;
            cut[first * count + last] = choice;
        }
    }
    if !cost[count - 1].is_finite() {
        return None;
    }
    let mut faces = Vec::with_capacity(count - 2);
    let mut pending = vec![(0usize, count - 1)];
    while let Some((first, last)) = pending.pop() {
        if last <= first + 1 {
            continue;
        }
        let middle = cut[first * count + last];
        faces.push([first, middle, last]);
        pending.push((first, middle));
        pending.push((middle, last));
    }
    Some(faces)
}

fn triangle_weight(
    points: &[DVec3],
    first: usize,
    middle: usize,
    last: usize,
    normal: DVec3,
    folded: bool,
) -> f64 {
    let [left, centre, right] = [points[first], points[middle], points[last]];
    let cross = (centre - left).cross(right - left);
    let area = 0.5 * cross.length();
    if !area.is_finite() || area <= AREA_FLOOR {
        return f64::INFINITY;
    }
    let alignment = (cross / cross.length()).dot(normal);
    if !folded && alignment <= 0.0 {
        return f64::INFINITY;
    }
    area * (1.0 + FOLD_PENALTY * (1.0 - alignment.clamp(-1.0, 1.0)))
}
