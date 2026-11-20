use std::collections::HashMap;

use glam::Vec3;

use crate::marching::{CORNER, EDGE_CORNER, EDGE_TABLE, TRIANGLE_TABLE};
use crate::mesh::MeshVertex;

const CELL_LIMIT: usize = 1 << 21;
const EPSILON: f32 = 1.0e-12;
const SAMPLES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    center: [f32; 3],
    radius: f32,
}

impl Ball {
    pub fn new(center: [f32; 3], radius: f32) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a ball centre is finite"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a ball radius is a positive length"
        );
        Self { center, radius }
    }

    pub fn center(self) -> [f32; 3] {
        self.center
    }

    pub fn radius(self) -> f32 {
        self.radius
    }
}

pub struct Union {
    balls: Vec<Ball>,
    cell: f32,
    origin: [f32; 3],
    members: HashMap<[i64; 3], Vec<u32>>,
}

impl Union {
    pub fn new(balls: Vec<Ball>) -> Option<Self> {
        let cell = balls
            .iter()
            .map(|ball| ball.radius)
            .fold(f32::INFINITY, f32::min);
        if !(cell.is_finite() && cell > 0.0) {
            return None;
        }
        let origin = [0, 1, 2].map(|axis| {
            balls
                .iter()
                .map(|ball| ball.center[axis] - ball.radius)
                .fold(f32::INFINITY, f32::min)
        });
        let mut members = HashMap::<[i64; 3], Vec<u32>>::new();
        for (index, ball) in balls.iter().enumerate() {
            let Some((low, high)) = bounds(origin, cell, ball) else {
                continue;
            };
            let cells = [0, 1, 2]
                .map(|axis| (high[axis] - low[axis] + 1) as usize)
                .iter()
                .product::<usize>();
            if members.len() + cells > CELL_LIMIT {
                return None;
            }
            for z in low[2]..=high[2] {
                for y in low[1]..=high[1] {
                    for x in low[0]..=high[0] {
                        members.entry([x, y, z]).or_default().push(index as u32);
                    }
                }
            }
        }
        Some(Self {
            balls,
            cell,
            origin,
            members,
        })
    }

    pub fn volume(&self) -> f64 {
        let cube = f64::from(self.cell).powi(3);
        let shift = [1, 3, 5, 7].map(|sample| sample as f32 / (2.0 * SAMPLES as f32));
        let mut total = 0.0f64;
        let mut candidates = Vec::new();
        for (key, members) in &self.members {
            candidates.clear();
            candidates.extend_from_slice(members);
            let anchor = self.anchor(*key);
            let mut inside = 0;
            for z in shift {
                for y in shift {
                    for x in shift {
                        let point = anchor + Vec3::new(x, y, z) * self.cell;
                        if self.field(point, &candidates) >= 0.0 {
                            inside += 1;
                        }
                    }
                }
            }
            total += cube * f64::from(inside) / (SAMPLES * SAMPLES * SAMPLES) as f64;
        }
        total
    }

    pub fn mesh(&self, object: u32) -> (Vec<MeshVertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut edges = HashMap::<([i64; 3], u8), u32>::new();
        let mut cells = self.members.keys().copied().collect::<Vec<_>>();
        cells.sort_unstable();
        let mut values = [0.0f32; 8];
        let mut candidates = Vec::new();
        for key in cells {
            candidates.clear();
            if let Some(held) = self.members.get(&key) {
                candidates.extend_from_slice(held);
            }
            let anchor = self.anchor(key);
            let mut bits = 0u32;
            for (corner, offset) in CORNER.iter().enumerate() {
                let point = anchor
                    + Vec3::new(offset[0] as f32, offset[1] as f32, offset[2] as f32) * self.cell;
                values[corner] = -self.field(point, &candidates);
                if values[corner] < 0.0 {
                    bits |= 1 << corner;
                }
            }
            if EDGE_TABLE[bits as usize] == 0 {
                continue;
            }
            let table = TRIANGLE_TABLE[bits as usize];
            let mut corner = 0;
            while corner < 16 && table[corner] >= 0 {
                let mut triangle = [0u32; 3];
                for (offset, vertex) in triangle.iter_mut().enumerate() {
                    let edge = EDGE_CORNER[table[corner + offset] as usize];
                    *vertex = self.vertex(
                        &mut vertices,
                        &mut edges,
                        &values,
                        &candidates,
                        anchor,
                        key,
                        edge,
                        object,
                    );
                }
                self.push(&vertices, &mut indices, triangle);
                corner += 3;
            }
        }
        (vertices, indices)
    }

    fn anchor(&self, key: [i64; 3]) -> Vec3 {
        Vec3::new(
            self.origin[0] + key[0] as f32 * self.cell,
            self.origin[1] + key[1] as f32 * self.cell,
            self.origin[2] + key[2] as f32 * self.cell,
        )
    }

    fn field(&self, point: Vec3, candidates: &[u32]) -> f32 {
        let mut best = f32::NEG_INFINITY;
        for candidate in candidates {
            let ball = &self.balls[*candidate as usize];
            let offset = point - Vec3::from(ball.center);
            best = best.max(ball.radius * ball.radius - offset.length_squared());
        }
        best
    }

    #[allow(clippy::too_many_arguments)]
    fn vertex(
        &self,
        vertices: &mut Vec<MeshVertex>,
        edges: &mut HashMap<([i64; 3], u8), u32>,
        values: &[f32; 8],
        candidates: &[u32],
        anchor: Vec3,
        key: [i64; 3],
        edge: [usize; 2],
        object: u32,
    ) -> u32 {
        let first = CORNER[edge[0]];
        let second = CORNER[edge[1]];
        let mut lower = key;
        let mut axis = 0u8;
        let mut forward = true;
        for component in 0..3 {
            lower[component] += first[component].min(second[component]);
            if first[component] != second[component] {
                axis = component as u8;
                forward = first[component] < second[component];
            }
        }
        if let Some(vertex) = edges.get(&(lower, axis)) {
            return *vertex;
        }
        let start = Vec3::new(first[0] as f32, first[1] as f32, first[2] as f32);
        let end = Vec3::new(second[0] as f32, second[1] as f32, second[2] as f32);
        let low = values[edge[0]];
        let high = values[edge[1]];
        let fraction = if (high - low).abs() > EPSILON {
            (-low / (high - low)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let fraction = if forward { fraction } else { 1.0 - fraction };
        let point = anchor + (start + (end - start) * fraction) * self.cell;
        let normal = self.normal(point, candidates, end - start);
        let vertex = vertices.len() as u32;
        vertices.push(MeshVertex::new(point.to_array(), normal.to_array(), object));
        edges.insert((lower, axis), vertex);
        vertex
    }

    fn normal(&self, point: Vec3, candidates: &[u32], fallback: Vec3) -> Vec3 {
        let mut best = f32::NEG_INFINITY;
        let mut slope = Vec3::ZERO;
        for candidate in candidates {
            let ball = &self.balls[*candidate as usize];
            let offset = point - Vec3::from(ball.center);
            let reach = ball.radius * ball.radius - offset.length_squared();
            if reach > best {
                best = reach;
                slope = -offset;
            }
        }
        if slope.length_squared() > 1.0e-20 {
            return slope.normalize();
        }
        if fallback.length_squared() > 1.0e-20 {
            return fallback.normalize();
        }
        Vec3::Z
    }

    fn push(&self, vertices: &[MeshVertex], indices: &mut Vec<u32>, triangle: [u32; 3]) {
        if triangle[0] == triangle[1] || triangle[1] == triangle[2] || triangle[2] == triangle[0] {
            return;
        }
        let positions = triangle.map(|index| Vec3::from(vertices[index as usize].position()));
        let geometric = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
        if geometric.length_squared() <= f32::MIN_POSITIVE {
            return;
        }
        let average = triangle
            .map(|index| Vec3::from(vertices[index as usize].normal()))
            .iter()
            .copied()
            .sum::<Vec3>();
        let mut ordering = triangle;
        if geometric.dot(average) < 0.0 {
            ordering.swap(1, 2);
        }
        indices.extend_from_slice(&ordering);
    }
}

fn bounds(origin: [f32; 3], cell: f32, ball: &Ball) -> Option<([i64; 3], [i64; 3])> {
    let mut low = [0i64; 3];
    let mut high = [0i64; 3];
    for axis in 0..3 {
        let start = ball.center[axis] - ball.radius;
        let end = ball.center[axis] + ball.radius;
        low[axis] = ((start - origin[axis]) / cell).floor() as i64;
        high[axis] = ((end - origin[axis]) / cell).ceil() as i64;
    }
    (low.iter().zip(high.iter()).all(|(low, high)| low <= high)).then_some((low, high))
}
