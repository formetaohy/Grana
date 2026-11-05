use std::ops::Range;

use glam::Vec3;
use grana_geometry::MeshVertex;

use super::tile::Contact;
use super::{EMPTY, OUTSIDE, Tile, find};
use crate::table::{EDGE_TABLE, TRIANGLE_TABLE};

const WORD: usize = 64;
const SHIFT: usize = 1;

const CORNER: [[i64; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];

const EDGE_CORNER: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

const DIRECTIONS: [[i64; 3]; 7] = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 1, 0],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];

impl Tile<'_> {
    pub(super) fn emit(&mut self, core: &[Range<u32>; 3]) -> Result<(), String> {
        let plan = self.pass.plan;
        let mut low = [0i64; 3];
        let mut high = [0i64; 3];
        for axis in 0..3 {
            low[axis] = if core[axis].start == plan.region[axis].start {
                core[axis].start as i64 - 1
            } else {
                core[axis].start as i64
            };
            high[axis] = core[axis].end as i64;
        }
        self.edges.clear();
        let band = self.band.take().expect("a surface tile holds its band");
        let start = self.start;
        let first = (low[0] - start[0] + SHIFT as i64) as usize;
        let last = (high[0] - start[0] + SHIFT as i64) as usize;
        let head = first / WORD;
        let tail = (last - 1) / WORD;
        let keep = (1u64 << (last % WORD).max(1)) - 1;
        for bz in low[2]..high[2] {
            let bitset_z = (bz - start[2] + SHIFT as i64) as usize;
            for by in low[1]..high[1] {
                let bitset_y = (by - start[1] + SHIFT as i64) as usize;
                for word in head..=tail {
                    let mut pending = band.active_word(bitset_y, bitset_z, word);
                    if word == head {
                        pending &= u64::MAX << (first % WORD);
                    }
                    if word == tail && !last.is_multiple_of(WORD) {
                        pending &= keep;
                    }
                    while pending != 0 {
                        let index = word * WORD + pending.trailing_zeros() as usize;
                        pending &= pending - 1;
                        self.cell(index as i64 - SHIFT as i64 + start[0], by, bz);
                    }
                }
            }
            if self.fragment_bytes() > super::TILE_FRAGMENT_LIMIT {
                return Err(format!(
                    "a surface tile exceeds the {} byte fragment limit",
                    super::TILE_FRAGMENT_LIMIT
                ));
            }
        }
        Ok(())
    }

    fn cell(&mut self, bx: i64, by: i64, bz: i64) {
        let threshold = self.pass.plan.threshold;
        let mut index = 0u32;
        let mut values = [0.0f32; 8];
        let mut corners = [[0i64; 3]; 8];
        let mut candidates = [EMPTY; 8];
        for (corner, offset) in CORNER.iter().enumerate() {
            let cell = [bx + offset[0], by + offset[1], bz + offset[2]];
            corners[corner] = cell;
            values[corner] = match self.value_index(cell) {
                Some(slot) => self.field[slot],
                None => OUTSIDE,
            };
            candidates[corner] = match self.mask_index(cell) {
                Some(slot) => self.labels[slot],
                None => EMPTY,
            };
            if values[corner] < threshold {
                index |= 1 << corner;
            }
        }
        if EDGE_TABLE[index as usize] == 0 {
            return;
        }
        let mut object = EMPTY;
        let mut deepest = threshold;
        for (corner, value) in values.iter().enumerate() {
            if *value < deepest || candidates[corner] == EMPTY {
                continue;
            }
            object = find(&mut self.nodes, candidates[corner]);
            deepest = *value;
        }
        if object == EMPTY {
            return;
        }
        self.record_contact(object, &candidates, &values);
        let table = TRIANGLE_TABLE[index as usize];
        let mut corner = 0;
        while corner < 16 && table[corner] >= 0 {
            let mut triangle = [0u32; 3];
            for (offset, vertex) in triangle.iter_mut().enumerate() {
                let edge = EDGE_CORNER[table[corner + offset] as usize];
                *vertex = self.edge_vertex(edge[0], edge[1], &values, &corners, object);
            }
            self.push_triangle(triangle, object);
            corner += 3;
        }
    }

    fn record_contact(&mut self, object: u32, candidates: &[u32; 8], values: &[f32; 8]) {
        let threshold = self.pass.plan.threshold;
        if self.nodes[object as usize].voxels > 0 {
            return;
        }
        let mut contact = Contact {
            object,
            labels: [EMPTY; 8],
            count: 0,
        };
        for corner in 0..8 {
            if values[corner] < threshold || candidates[corner] == EMPTY {
                continue;
            }
            contact.labels[contact.count as usize] = candidates[corner];
            contact.count += 1;
        }
        if contact.count > 0 {
            self.contacts.push(contact);
        }
    }

    fn edge_vertex(
        &mut self,
        first: usize,
        second: usize,
        values: &[f32; 8],
        corners: &[[i64; 3]; 8],
        object: u32,
    ) -> u32 {
        let plan = self.pass.plan;
        let mut direction = [0i64; 3];
        let mut lower = [0i64; 3];
        for axis in 0..3 {
            direction[axis] = (corners[second][axis] - corners[first][axis]).abs();
            lower[axis] = corners[first][axis].min(corners[second][axis]);
        }
        let slot = DIRECTIONS
            .iter()
            .position(|candidate| *candidate == direction)
            .expect("a marching cubes edge runs along one of seven directions");
        let extent = plan.extent;
        let linear = lower[0] + i64::from(extent[0]) * (lower[1] + i64::from(extent[1]) * lower[2]);
        let key = ((linear as u64) << 4) | slot as u64;
        if let Some(vertex) = self.edges.get(&key) {
            return *vertex;
        }
        let start = if corners[first] == lower {
            first
        } else {
            second
        };
        let end = if start == first { second } else { first };
        let low = values[start];
        let high = values[end];
        let threshold = plan.threshold;
        let fraction = if (high - low).abs() > f32::EPSILON {
            ((threshold - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let point = [0, 1, 2].map(|axis| lower[axis] as f32 + direction[axis] as f32 * fraction);
        let fallback = Vec3::new(
            direction[0] as f32,
            direction[1] as f32,
            direction[2] as f32,
        );
        let fallback = if low < high { -fallback } else { fallback };
        let normal = self.normal(point, fallback);
        let world = [0, 1, 2].map(|component| {
            (point[component] + 0.5) * plan.spacing[component] - 0.5 * plan.world[component]
        });
        let vertex = self.vertices.len() as u32;
        self.vertices
            .push(MeshVertex::new(world, normal.to_array(), object));
        self.keys.push(key);
        self.edges.insert(key, vertex);
        vertex
    }

    fn push_triangle(&mut self, triangle: [u32; 3], object: u32) {
        if triangle[0] == triangle[1] || triangle[1] == triangle[2] || triangle[2] == triangle[0] {
            return;
        }
        let positions = triangle.map(|index| self.vertices[index as usize].position());
        let geometric = (Vec3::from(positions[1]) - Vec3::from(positions[0]))
            .cross(Vec3::from(positions[2]) - Vec3::from(positions[0]));
        let area = 0.5 * geometric.length();
        if !area.is_finite() || area <= 0.0 {
            return;
        }
        let average = triangle
            .map(|index| Vec3::from(self.vertices[index as usize].normal()))
            .iter()
            .copied()
            .sum::<Vec3>();
        let mut ordering = [triangle[0], triangle[1], triangle[2]];
        if geometric.dot(average) < 0.0 {
            ordering.swap(1, 2);
        }
        self.indices.extend_from_slice(&ordering);
        self.triangles.push(object);
    }

    fn normal(&self, point: [f32; 3], fallback: Vec3) -> Vec3 {
        let spacing = self.pass.plan.spacing;
        let gradient = Vec3::new(
            (self.sample([point[0] + 1.0, point[1], point[2]])
                - self.sample([point[0] - 1.0, point[1], point[2]]))
                / (2.0 * spacing[0]),
            (self.sample([point[0], point[1] + 1.0, point[2]])
                - self.sample([point[0], point[1] - 1.0, point[2]]))
                / (2.0 * spacing[1]),
            (self.sample([point[0], point[1], point[2] + 1.0])
                - self.sample([point[0], point[1], point[2] - 1.0]))
                / (2.0 * spacing[2]),
        );
        if gradient.length_squared() > 1.0e-20 {
            return -gradient.normalize();
        }
        if fallback.length_squared() > 1.0e-20 {
            return fallback.normalize();
        }
        Vec3::Z
    }

    fn sample(&self, point: [f32; 3]) -> f32 {
        let mut low = [0i64; 3];
        let mut fraction = [0.0f32; 3];
        for axis in 0..3 {
            let value = point[axis].floor();
            low[axis] = value as i64;
            fraction[axis] = (point[axis] - value).clamp(0.0, 1.0);
        }
        let mut total = 0.0;
        for corner in 0..8 {
            let offset = [
                (corner & 1) as i64,
                ((corner >> 1) & 1) as i64,
                ((corner >> 2) & 1) as i64,
            ];
            let share = [0, 1, 2].map(|axis| {
                if offset[axis] == 0 {
                    1.0 - fraction[axis]
                } else {
                    fraction[axis]
                }
            });
            let cell = [low[0] + offset[0], low[1] + offset[1], low[2] + offset[2]];
            let value = match self.value_index(cell) {
                Some(slot) => self.field[slot],
                None => OUTSIDE,
            };
            total += value * share[0] * share[1] * share[2];
        }
        total
    }
}
