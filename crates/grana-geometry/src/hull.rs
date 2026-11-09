use std::cmp::Ordering;
use std::collections::HashMap;

use glam::DVec3;

const RELATIVE_EPSILON: f64 = 1.0e-9;

pub struct Hull {
    vertices: Vec<DVec3>,
    faces: Vec<[usize; 3]>,
}

impl Hull {
    pub fn of(points: &[[f32; 3]]) -> Option<Self> {
        let unique = dedupe(points)?;
        if unique.len() < 4 {
            return None;
        }
        let extent = unique
            .iter()
            .map(|point| point.length())
            .fold(0.0f64, f64::max)
            .max(1.0);
        let epsilon = RELATIVE_EPSILON * extent;
        let simplex = simplex(&unique, epsilon)?;
        let mut faces = seed(&unique, simplex);
        let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
        for (slot, face) in faces.iter().enumerate() {
            for edge in face.edges() {
                edges.insert(edge, slot);
            }
        }
        let mut pending = Vec::new();
        let mut queued = vec![false; faces.len()];
        for (index, point) in unique.iter().enumerate() {
            if simplex.contains(&index) {
                continue;
            }
            if let Some((slot, distance)) = nearest(&faces, *point, epsilon) {
                faces[slot].push(index, distance);
                if !queued[slot] {
                    queued[slot] = true;
                    pending.push(slot);
                }
            }
        }
        let mut marks = vec![0u32; faces.len()];
        let mut generation = 0u32;
        while let Some(slot) = pending.pop() {
            queued[slot] = false;
            if faces[slot].retired {
                continue;
            }
            let Some((distance, apex)) = faces[slot].farthest else {
                continue;
            };
            if distance <= epsilon {
                continue;
            }
            let point = unique[apex];
            generation += 1;
            let mut visible = Vec::new();
            let mut stack = vec![slot];
            while let Some(current) = stack.pop() {
                let face = &faces[current];
                if face.retired || marks[current] == generation || face.distance(point) <= epsilon {
                    continue;
                }
                marks[current] = generation;
                visible.push(current);
                for (from, to) in face.edges() {
                    if let Some(neighbor) = edges.get(&(to, from))
                        && !faces[*neighbor].retired
                        && marks[*neighbor] != generation
                    {
                        stack.push(*neighbor);
                    }
                }
            }
            let mut horizon = Vec::new();
            let mut carried = Vec::new();
            for current in visible {
                for edge in faces[current].edges() {
                    let opposite = edges.get(&(edge.1, edge.0)).copied();
                    if opposite.is_some_and(|opposite| marks[opposite] != generation) {
                        horizon.push(edge);
                    }
                    edges.remove(&edge);
                }
                let face = &mut faces[current];
                carried.append(&mut face.outside);
                face.retired = true;
                face.farthest = None;
            }
            let mut created = Vec::new();
            for (from, to) in horizon {
                if let Some(face) = Face::new([from, to, apex], &unique, epsilon) {
                    let id = faces.len();
                    faces.push(face);
                    for edge in faces[id].edges() {
                        edges.insert(edge, id);
                    }
                    marks.push(0);
                    queued.push(false);
                    created.push(id);
                }
            }
            for index in carried {
                if index == apex {
                    continue;
                }
                let point = unique[index];
                let mut best = epsilon;
                let mut target = None;
                for id in &created {
                    let distance = faces[*id].distance(point);
                    if distance > best {
                        best = distance;
                        target = Some(*id);
                    }
                }
                if let Some(id) = target {
                    faces[id].push(index, best);
                    if !queued[id] {
                        queued[id] = true;
                        pending.push(id);
                    }
                }
            }
        }
        Self::assemble(unique, faces)
    }

    fn assemble(unique: Vec<DVec3>, faces: Vec<Face>) -> Option<Self> {
        let alive = faces
            .into_iter()
            .filter(|face| !face.retired)
            .collect::<Vec<_>>();
        if alive.len() < 4 {
            return None;
        }
        let mut used = vec![None; unique.len()];
        let mut vertices = Vec::new();
        let mut triangles = Vec::with_capacity(alive.len());
        for face in &alive {
            let mut corners = [0usize; 3];
            for (corner, index) in face.corners.iter().enumerate() {
                corners[corner] = match used[*index] {
                    Some(slot) => slot,
                    None => {
                        let slot = vertices.len();
                        used[*index] = Some(slot);
                        vertices.push(unique[*index]);
                        slot
                    }
                };
            }
            triangles.push(corners);
        }
        Some(Self {
            vertices,
            faces: triangles,
        })
    }

    pub fn volume(&self) -> f64 {
        let origin = self.vertices[self.faces[0][0]];
        self.faces
            .iter()
            .map(|face| {
                let [a, b, c] = face.map(|index| self.vertices[index] - origin);
                a.dot(b.cross(c)) / 6.0
            })
            .sum::<f64>()
            .abs()
    }

    pub fn diameter(&self) -> f64 {
        let tree = Tree::new(&self.vertices);
        self.vertices
            .iter()
            .map(|point| tree.farthest(&self.vertices, *point).sqrt())
            .fold(0.0f64, f64::max)
    }
}

struct Face {
    corners: [usize; 3],
    normal: DVec3,
    offset: f64,
    outside: Vec<usize>,
    farthest: Option<(f64, usize)>,
    retired: bool,
}

impl Face {
    fn new(corners: [usize; 3], points: &[DVec3], epsilon: f64) -> Option<Self> {
        let [a, b, c] = corners.map(|index| points[index]);
        let normal = (b - a).cross(c - a);
        let length = normal.length();
        if length <= epsilon * epsilon {
            return None;
        }
        let normal = normal / length;
        let offset = normal.dot(a);
        Some(Self {
            corners,
            normal,
            offset,
            outside: Vec::new(),
            farthest: None,
            retired: false,
        })
    }

    fn distance(&self, point: DVec3) -> f64 {
        self.normal.dot(point) - self.offset
    }

    fn push(&mut self, index: usize, distance: f64) {
        if self.farthest.is_none_or(|(held, _)| distance > held) {
            self.farthest = Some((distance, index));
        }
        self.outside.push(index);
    }

    fn edges(&self) -> [(usize, usize); 3] {
        [
            (self.corners[0], self.corners[1]),
            (self.corners[1], self.corners[2]),
            (self.corners[2], self.corners[0]),
        ]
    }
}

fn dedupe(points: &[[f32; 3]]) -> Option<Vec<DVec3>> {
    let mut unique = Vec::with_capacity(points.len());
    let mut seen = HashMap::with_capacity(points.len());
    for point in points {
        if point.iter().any(|value| !value.is_finite()) {
            return None;
        }
        if seen.insert(point.map(f32::to_bits), ()).is_none() {
            unique.push(DVec3::new(
                f64::from(point[0]),
                f64::from(point[1]),
                f64::from(point[2]),
            ));
        }
    }
    Some(unique)
}

fn simplex(points: &[DVec3], epsilon: f64) -> Option<[usize; 4]> {
    let first = (0..points.len())
        .min_by(|left, right| lexicographic(points[*left], points[*right]))
        .expect("a hull holds a point");
    let second = farthest_from(points, |point| (point - points[first]).length());
    if (points[second] - points[first]).length() <= epsilon {
        return None;
    }
    let base = points[second] - points[first];
    let third = farthest_from(points, |point| {
        (point - points[first]).cross(base).length() / base.length()
    });
    if (points[third] - points[first]).cross(base).length() / base.length() <= epsilon {
        return None;
    }
    let normal = (points[second] - points[first]).cross(points[third] - points[first]);
    let fourth = farthest_from(points, |point| {
        (point - points[first]).dot(normal).abs() / normal.length()
    });
    if (points[fourth] - points[first]).dot(normal).abs() / normal.length() <= epsilon {
        return None;
    }
    Some([first, second, third, fourth])
}

fn farthest_from(points: &[DVec3], measure: impl Fn(DVec3) -> f64) -> usize {
    let mut best = 0usize;
    let mut held = f64::NEG_INFINITY;
    for (index, point) in points.iter().enumerate() {
        let value = measure(*point);
        if value > held {
            held = value;
            best = index;
        }
    }
    best
}

fn lexicographic(left: DVec3, right: DVec3) -> Ordering {
    left.x
        .total_cmp(&right.x)
        .then_with(|| left.y.total_cmp(&right.y))
        .then_with(|| left.z.total_cmp(&right.z))
}

fn seed(points: &[DVec3], simplex: [usize; 4]) -> Vec<Face> {
    let mut faces = Vec::with_capacity(4);
    for opposite in 0..4 {
        let corners = match opposite {
            0 => [simplex[1], simplex[2], simplex[3]],
            1 => [simplex[0], simplex[2], simplex[3]],
            2 => [simplex[0], simplex[1], simplex[3]],
            _ => [simplex[0], simplex[1], simplex[2]],
        };
        let mut face = Face::new(corners, points, 0.0).expect("a simplex face is not flat");
        if face.distance(points[simplex[opposite]]) > 0.0 {
            face = Face::new([corners[0], corners[2], corners[1]], points, 0.0)
                .expect("a reversed simplex face is not flat");
        }
        faces.push(face);
    }
    faces
}

fn nearest(faces: &[Face], point: DVec3, epsilon: f64) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (slot, face) in faces.iter().enumerate() {
        let distance = face.distance(point);
        if distance > best.map_or(epsilon, |(_, held)| held) {
            best = Some((slot, distance));
        }
    }
    best
}

struct Tree {
    nodes: Vec<Node>,
    root: Option<usize>,
}

struct Node {
    point: usize,
    left: Option<usize>,
    right: Option<usize>,
    low: DVec3,
    high: DVec3,
}

impl Tree {
    fn new(points: &[DVec3]) -> Self {
        let mut tree = Self {
            nodes: Vec::with_capacity(points.len()),
            root: None,
        };
        let mut order = (0..points.len()).collect::<Vec<_>>();
        tree.root = tree.build(points, &mut order);
        tree
    }

    fn build(&mut self, points: &[DVec3], order: &mut [usize]) -> Option<usize> {
        if order.is_empty() {
            return None;
        }
        let mut low = DVec3::splat(f64::INFINITY);
        let mut high = DVec3::splat(f64::NEG_INFINITY);
        for index in order.iter() {
            low = low.min(points[*index]);
            high = high.max(points[*index]);
        }
        let spread = high - low;
        let axis = if spread.x >= spread.y && spread.x >= spread.z {
            0
        } else if spread.y >= spread.z {
            1
        } else {
            2
        };
        order.sort_by(|left, right| {
            points[*left][axis]
                .total_cmp(&points[*right][axis])
                .then_with(|| left.cmp(right))
        });
        let middle = order.len() / 2;
        let point = order[middle];
        let (near, far) = order.split_at_mut(middle);
        let left = self.build(points, near);
        let right = self.build(points, &mut far[1..]);
        let slot = self.nodes.len();
        self.nodes.push(Node {
            point,
            left,
            right,
            low,
            high,
        });
        Some(slot)
    }

    fn farthest(&self, points: &[DVec3], point: DVec3) -> f64 {
        let Some(root) = self.root else {
            return 0.0;
        };
        let mut best = 0.0f64;
        let mut stack = vec![root];
        while let Some(slot) = stack.pop() {
            let node = &self.nodes[slot];
            let distance = (points[node.point] - point).length_squared();
            if distance > best {
                best = distance;
            }
            for child in [node.left, node.right].into_iter().flatten() {
                if bound(&self.nodes[child], point) > best {
                    stack.push(child);
                }
            }
        }
        best
    }
}

fn bound(node: &Node, point: DVec3) -> f64 {
    let low = node.low.min(point);
    let high = node.high.max(point);
    (high - low).length_squared()
}
