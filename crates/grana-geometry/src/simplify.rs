use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::{DMat3, DVec3, Vec3};

use crate::{Mesh, MeshVertex};

const MIN_TRIANGLES: usize = 4;
const BOUNDARY_WEIGHT: f64 = 1.0e3;
const DETERMINANT_FLOOR: f64 = 1.0e-14;
const COST_FLOOR: f64 = 1.0e-12;

pub fn simplify(mesh: &Mesh, target: usize) -> Mesh {
    assert!(target > 0, "a simplified mesh keeps triangles");
    if mesh.triangle_count() <= target {
        return mesh.clone();
    }
    let parts = split(mesh);
    let ratio = target as f64 / mesh.triangle_count() as f64;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, part) in parts.iter().enumerate() {
        let wanted = ((part.triangles.len() as f64 * ratio).round() as usize)
            .max(MIN_TRIANGLES.min(part.triangles.len()));
        let (positions, triangles) = collapse(part, wanted);
        let normals = normals(&positions, &triangles);
        let base = vertices.len() as u32;
        vertices.extend(positions.iter().zip(&normals).map(|(position, normal)| {
            MeshVertex::new(position.to_array(), normal.to_array(), object as u32)
        }));
        indices.extend(triangles.iter().flatten().map(|index| base + index));
    }
    Mesh::new(vertices, indices)
}

struct Part {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

fn split(mesh: &Mesh) -> Vec<Part> {
    let objects = mesh
        .vertices()
        .iter()
        .map(|vertex| vertex.object() as usize)
        .max()
        .map_or(0, |object| object + 1);
    let mut parts = (0..objects)
        .map(|_| Part {
            positions: Vec::new(),
            triangles: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut local = vec![u32::MAX; mesh.vertex_count()];
    for triangle in mesh.indices().as_chunks::<3>().0 {
        let object = mesh.vertices()[triangle[0] as usize].object() as usize;
        let part = &mut parts[object];
        let corners = triangle.map(|index| {
            let slot = &mut local[index as usize];
            if *slot == u32::MAX {
                *slot = part.positions.len() as u32;
                part.positions
                    .push(Vec3::from(mesh.vertices()[index as usize].position()));
            }
            *slot
        });
        part.triangles.push(corners);
    }
    parts.retain(|part| !part.triangles.is_empty());
    parts
}

#[derive(Clone, Copy, Default)]
struct Quadric {
    a00: f64,
    a01: f64,
    a02: f64,
    a03: f64,
    a11: f64,
    a12: f64,
    a13: f64,
    a22: f64,
    a23: f64,
    a33: f64,
}

impl Quadric {
    fn plane(normal: Vec3, point: Vec3, weight: f64) -> Self {
        let normal = normal.normalize_or_zero();
        if normal == Vec3::ZERO || weight <= 0.0 {
            return Self::default();
        }
        let offset = -normal.dot(point) as f64;
        let [x, y, z] = normal.to_array().map(f64::from);
        Self {
            a00: weight * x * x,
            a01: weight * x * y,
            a02: weight * x * z,
            a03: weight * x * offset,
            a11: weight * y * y,
            a12: weight * y * z,
            a13: weight * y * offset,
            a22: weight * z * z,
            a23: weight * z * offset,
            a33: weight * offset * offset,
        }
    }

    fn add(&mut self, other: &Self) {
        self.a00 += other.a00;
        self.a01 += other.a01;
        self.a02 += other.a02;
        self.a03 += other.a03;
        self.a11 += other.a11;
        self.a12 += other.a12;
        self.a13 += other.a13;
        self.a22 += other.a22;
        self.a23 += other.a23;
        self.a33 += other.a33;
    }

    fn sum(&self, other: &Self) -> Self {
        let mut sum = *self;
        sum.add(other);
        sum
    }

    fn error(self, point: Vec3) -> f64 {
        let [x, y, z] = point.to_array().map(f64::from);
        self.a00 * x * x
            + 2.0 * self.a01 * x * y
            + 2.0 * self.a02 * x * z
            + 2.0 * self.a03 * x
            + self.a11 * y * y
            + 2.0 * self.a12 * y * z
            + 2.0 * self.a13 * y
            + self.a22 * z * z
            + 2.0 * self.a23 * z
            + self.a33
    }

    fn optimum(self) -> Option<Vec3> {
        let matrix = DMat3::from_cols(
            DVec3::new(self.a00, self.a01, self.a02),
            DVec3::new(self.a01, self.a11, self.a12),
            DVec3::new(self.a02, self.a12, self.a22),
        );
        if matrix.determinant().abs() <= DETERMINANT_FLOOR {
            return None;
        }
        let target = DVec3::new(-self.a03, -self.a13, -self.a23);
        let solution = matrix.inverse() * target;
        solution
            .is_finite()
            .then(|| Vec3::new(solution.x as f32, solution.y as f32, solution.z as f32))
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    position: Vec3,
    quadric: Quadric,
    alive: bool,
    version: u32,
}

#[derive(Clone, Copy)]
struct Candidate {
    cost: f64,
    a: u32,
    b: u32,
    version_a: u32,
    version_b: u32,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cost.to_bits() == other.cost.to_bits()
            && self.a == other.a
            && self.b == other.b
            && self.version_a == other.version_a
            && self.version_b == other.version_b
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.a.cmp(&self.a))
            .then_with(|| other.b.cmp(&self.b))
    }
}

fn collapse(part: &Part, target: usize) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let count = part.positions.len();
    let mut triangles = part.triangles.clone();
    if triangles.len() <= target {
        return (part.positions.clone(), triangles);
    }
    let boundary = boundary_vertices(&triangles);
    let mut vertices = (0..count)
        .map(|index| Vertex {
            position: part.positions[index],
            quadric: Quadric::default(),
            alive: true,
            version: 0,
        })
        .collect::<Vec<_>>();
    for triangle in &triangles {
        let points = triangle.map(|index| part.positions[index as usize]);
        let normal = (points[1] - points[0]).cross(points[2] - points[0]);
        let area = 0.5 * normal.length() as f64;
        let quadric = Quadric::plane(normal, points[0], area);
        for index in triangle {
            vertices[*index as usize].quadric.add(&quadric);
        }
    }
    add_boundary_quadrics(&part.positions, &triangles, &mut vertices);
    let mut incident = vec![Vec::new(); count];
    let mut alive = vec![true; triangles.len()];
    for (index, triangle) in triangles.iter().enumerate() {
        for corner in triangle {
            incident[*corner as usize].push(index as u32);
        }
    }
    let mut heap = BinaryHeap::new();
    let mut seen = HashSet::new();
    for triangle in &triangles {
        for pair in [[0, 1], [1, 2], [2, 0]] {
            let (a, b) = ordered(triangle[pair[0]], triangle[pair[1]]);
            if !seen.insert((a, b)) || boundary.contains(&a) || boundary.contains(&b) {
                continue;
            }
            push(&mut heap, &vertices, a, b);
        }
    }
    let mut alive_triangles = triangles.len();
    while alive_triangles > target {
        let Some(candidate) = heap.pop() else {
            break;
        };
        let a = candidate.a;
        let b = candidate.b;
        if !vertices[a as usize].alive || !vertices[b as usize].alive {
            continue;
        }
        if vertices[a as usize].version != candidate.version_a
            || vertices[b as usize].version != candidate.version_b
        {
            continue;
        }
        let shared = shared_triangles(&triangles, &incident, &alive, a, b);
        if shared.len() != 2 {
            continue;
        }
        let Some((position, quadric)) = placement(&vertices, a, b) else {
            continue;
        };
        if common_neighbours(&triangles, &incident, &alive, a, b) != shared.len() {
            continue;
        }
        if !preserves_normals(&triangles, &incident, &alive, &vertices, a, b, position) {
            continue;
        }
        vertices[a as usize].position = position;
        vertices[a as usize].quadric = quadric;
        vertices[a as usize].version += 1;
        vertices[b as usize].alive = false;
        vertices[b as usize].version += 1;
        for index in &shared {
            alive[*index as usize] = false;
        }
        alive_triangles -= shared.len();
        let replaced = incident[b as usize].clone();
        for index in replaced {
            if !alive[index as usize] {
                continue;
            }
            let triangle = &mut triangles[index as usize];
            for corner in triangle {
                if *corner == b {
                    *corner = a;
                }
            }
            incident[a as usize].push(index);
        }
        incident[b as usize].clear();
        let neighbours = neighbours(&triangles, &incident, &alive, a);
        for neighbour in neighbours {
            push(&mut heap, &vertices, a, neighbour);
        }
    }
    compact(&vertices, &triangles, &alive)
}

fn ordered(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn boundary_vertices(triangles: &[[u32; 3]]) -> HashSet<u32> {
    let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
    for triangle in triangles {
        for pair in [[0, 1], [1, 2], [2, 0]] {
            let edge = ordered(triangle[pair[0]], triangle[pair[1]]);
            *counts.entry(edge).or_default() += 1;
        }
    }
    counts
        .iter()
        .filter(|(_, count)| **count == 1)
        .flat_map(|(edge, _)| [edge.0, edge.1])
        .collect()
}

fn add_boundary_quadrics(positions: &[Vec3], triangles: &[[u32; 3]], vertices: &mut [Vertex]) {
    let mut borders: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        for pair in [[0, 1], [1, 2], [2, 0]] {
            let edge = ordered(triangle[pair[0]], triangle[pair[1]]);
            borders
                .entry(edge)
                .and_modify(|entry| entry.1 += 1)
                .or_insert((index as u32, 1));
        }
    }
    for (edge, (triangle, count)) in borders {
        if count != 1 {
            continue;
        }
        let triangle = triangles[triangle as usize];
        let points = triangle.map(|index| positions[index as usize]);
        let face = (points[1] - points[0]).cross(points[2] - points[0]);
        let along = positions[edge.1 as usize] - positions[edge.0 as usize];
        let across = along.cross(face);
        let weight = BOUNDARY_WEIGHT * along.length() as f64;
        let quadric = Quadric::plane(across, positions[edge.0 as usize], weight);
        vertices[edge.0 as usize].quadric.add(&quadric);
        vertices[edge.1 as usize].quadric.add(&quadric);
    }
}

fn placement(vertices: &[Vertex], a: u32, b: u32) -> Option<(Vec3, Quadric)> {
    let left = vertices[a as usize];
    let right = vertices[b as usize];
    let quadric = left.quadric.sum(&right.quadric);
    let midpoint = 0.5 * (left.position + right.position);
    let mut best = midpoint;
    let mut error = quadric.error(midpoint);
    for point in [left.position, right.position] {
        let candidate = quadric.error(point);
        if candidate < error {
            error = candidate;
            best = point;
        }
    }
    if let Some(optimum) = quadric.optimum() {
        let candidate = quadric.error(optimum);
        if candidate.is_finite() && candidate < error {
            error = candidate;
            best = optimum;
        }
    }
    if !best.is_finite() || !error.is_finite() {
        return None;
    }
    Some((best, quadric))
}

fn push(heap: &mut BinaryHeap<Candidate>, vertices: &[Vertex], a: u32, b: u32) {
    if a == b || !vertices[a as usize].alive || !vertices[b as usize].alive {
        return;
    }
    let Some((position, quadric)) = placement(vertices, a, b) else {
        return;
    };
    let cost = quadric.error(position);
    let candidate = Candidate {
        cost: cost.max(COST_FLOOR),
        a,
        b,
        version_a: vertices[a as usize].version,
        version_b: vertices[b as usize].version,
    };
    heap.push(candidate);
}

fn shared_triangles(
    triangles: &[[u32; 3]],
    incident: &[Vec<u32>],
    alive: &[bool],
    a: u32,
    b: u32,
) -> Vec<u32> {
    incident[a as usize]
        .iter()
        .copied()
        .filter(|index| alive[*index as usize] && triangles[*index as usize].contains(&b))
        .collect()
}

fn neighbours(
    triangles: &[[u32; 3]],
    incident: &[Vec<u32>],
    alive: &[bool],
    vertex: u32,
) -> Vec<u32> {
    let mut found = Vec::new();
    for index in &incident[vertex as usize] {
        if !alive[*index as usize] {
            continue;
        }
        for corner in triangles[*index as usize] {
            if corner != vertex && !found.contains(&corner) {
                found.push(corner);
            }
        }
    }
    found
}

fn common_neighbours(
    triangles: &[[u32; 3]],
    incident: &[Vec<u32>],
    alive: &[bool],
    a: u32,
    b: u32,
) -> usize {
    let around = neighbours(triangles, incident, alive, a);
    let mut common = 0;
    for candidate in around {
        if candidate == a || candidate == b {
            continue;
        }
        if incident[b as usize]
            .iter()
            .any(|index| alive[*index as usize] && triangles[*index as usize].contains(&candidate))
        {
            common += 1;
        }
    }
    common
}

fn preserves_normals(
    triangles: &[[u32; 3]],
    incident: &[Vec<u32>],
    alive: &[bool],
    vertices: &[Vertex],
    a: u32,
    b: u32,
    position: Vec3,
) -> bool {
    let corner = |index: u32| vertices[index as usize].position;
    let moves = |index: u32| {
        if index == a || index == b {
            position
        } else {
            corner(index)
        }
    };
    for vertex in [a, b] {
        for index in &incident[vertex as usize] {
            let triangle = triangles[*index as usize];
            if !alive[*index as usize] || triangle.contains(&(if vertex == a { b } else { a })) {
                continue;
            }
            let before = {
                let points = triangle.map(corner);
                (points[1] - points[0]).cross(points[2] - points[0])
            };
            let after = {
                let points = triangle.map(moves);
                (points[1] - points[0]).cross(points[2] - points[0])
            };
            if after.length_squared() <= f32::MIN_POSITIVE {
                return false;
            }
            if before.dot(after) <= 0.0 {
                return false;
            }
        }
    }
    true
}

fn compact(
    vertices: &[Vertex],
    triangles: &[[u32; 3]],
    alive: &[bool],
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut remap = vec![u32::MAX; vertices.len()];
    let mut positions = Vec::new();
    let mut kept = Vec::new();
    for (index, triangle) in triangles.iter().enumerate() {
        if !alive[index] {
            continue;
        }
        let corners = triangle.map(|value| {
            let slot = &mut remap[value as usize];
            if *slot == u32::MAX {
                *slot = positions.len() as u32;
                positions.push(vertices[value as usize].position);
            }
            *slot
        });
        if corners[0] == corners[1] || corners[1] == corners[2] || corners[2] == corners[0] {
            continue;
        }
        kept.push(corners);
    }
    (positions, kept)
}

fn normals(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in triangles {
        let points = triangle.map(|index| positions[index as usize]);
        let normal = (points[1] - points[0]).cross(points[2] - points[0]);
        for index in triangle {
            normals[*index as usize] += normal;
        }
    }
    normals
        .into_iter()
        .map(|normal| {
            if normal.length_squared() > 0.0 {
                normal.normalize()
            } else {
                Vec3::Z
            }
        })
        .collect()
}
