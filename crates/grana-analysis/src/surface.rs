use std::collections::HashMap;

use glam::Vec3;
use grana_object::{
    Bounds, IntensityTable, Mesh, MeshVertex, SampleAccumulator, SurfaceSet, SurfaceStatistics,
};
use grana_volume::{BRICK_SIDE, BrickCoord, BrickData, BrickGrid, Level, PADDED_SIDE, Region};

use crate::SurfaceParams;
use crate::table::{EDGE_TABLE, TRIANGLE_TABLE};

const CORNER: [[usize; 3]; 8] = [
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

const EDGE_AXIS: [usize; 12] = [0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2];

const PADDED: usize = PADDED_SIDE as usize;
const PADDED_VOXELS: usize = PADDED * PADDED * PADDED;
const FACE_SIDE: usize = 33;
const EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Node {
    parent: u32,
    rank: u8,
    voxels: u64,
    sum: [f64; 3],
    low: [u32; 3],
    high: [u32; 3],
    area: f64,
    triangles: u64,
}

impl Node {
    fn new() -> Self {
        Self {
            parent: 0,
            rank: 0,
            voxels: 0,
            sum: [0.0; 3],
            low: [u32::MAX; 3],
            high: [0; 3],
            area: 0.0,
            triangles: 0,
        }
    }
}

pub struct SurfaceBuilder {
    level_index: usize,
    detection: usize,
    channels: usize,
    extent: [u32; 3],
    stride: [u64; 3],
    spacing: [f32; 3],
    world: [f32; 3],
    voxel_volume: f64,
    grid: BrickGrid,
    params: SurfaceParams,
    next: usize,
    first: [u32; 3],
    counts: [usize; 3],
    grid_counts: [usize; 3],
    ranges: [std::ops::Range<u32>; 3],
    values: Vec<Vec<f32>>,
    gradients: Vec<[f32; 3]>,
    mask: Vec<bool>,
    labels: Vec<u32>,
    nodes: Vec<Node>,
    accumulators: Vec<SampleAccumulator>,
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
    edges: HashMap<u64, u32>,
    x_face: Option<Vec<u32>>,
    y_face: Vec<Option<Vec<u32>>>,
    z_face: Vec<Option<Vec<u32>>>,
}

impl SurfaceBuilder {
    pub fn new(
        level_index: usize,
        level: Level,
        params: SurfaceParams,
        channels: usize,
        detection: usize,
        region: Region,
    ) -> Self {
        assert!(
            channels > 0 && detection < channels,
            "the detection channel is one of the measured channels"
        );
        let extent = level.extent();
        let grid = BrickGrid::new(extent);
        let grid_counts = grid.counts().map(|count| count as usize);
        let (ranges, first, counts) = match region.voxel_range(level) {
            Some(ranges) => {
                let first = [0, 1, 2].map(|axis| ranges[axis].start / BRICK_SIDE);
                let last = [0, 1, 2].map(|axis| (ranges[axis].end - 1) / BRICK_SIDE);
                let counts = [0, 1, 2].map(|axis| (last[axis] - first[axis] + 1) as usize);
                (ranges, first, counts)
            }
            None => ([0..0, 0..0, 0..0], [0; 3], [0; 3]),
        };
        let world = level.world_extent();
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let extent = [extent.x(), extent.y(), extent.z()];
        let stride = [
            1,
            u64::from(extent[0]),
            u64::from(extent[0]) * u64::from(extent[1]),
        ];
        let voxel_volume = f64::from(spacing[0]) * f64::from(spacing[1]) * f64::from(spacing[2]);
        Self {
            level_index,
            detection,
            channels,
            extent,
            stride,
            spacing,
            world,
            voxel_volume,
            grid,
            params,
            next: 0,
            first,
            counts,
            grid_counts,
            ranges,
            values: vec![vec![0.0; PADDED_VOXELS]; channels],
            gradients: vec![[0.0; 3]; PADDED_VOXELS],
            mask: Vec::new(),
            labels: Vec::new(),
            nodes: Vec::new(),
            accumulators: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            edges: HashMap::new(),
            x_face: None,
            y_face: vec![None; grid_counts[0]],
            z_face: vec![None; grid_counts[0] * grid_counts[1]],
        }
    }

    pub fn coord_count(&self) -> usize {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    pub fn coord(&self, index: usize) -> BrickCoord {
        assert!(index < self.coord_count(), "the brick index is in bounds");
        let x = index % self.counts[0];
        let y = (index / self.counts[0]) % self.counts[1];
        let z = index / (self.counts[0] * self.counts[1]);
        BrickCoord::new(
            self.first[0] + x as u32,
            self.first[1] + y as u32,
            self.first[2] + z as u32,
        )
    }

    pub fn add(&mut self, coord: BrickCoord, data: &BrickData) {
        assert_eq!(
            coord,
            self.coord(self.next),
            "surface bricks arrive in raster order"
        );
        assert_eq!(
            data.channels().len(),
            self.channels,
            "a surface brick carries every measured channel"
        );
        for (source, values) in data.channels().iter().zip(self.values.iter_mut()) {
            assert_eq!(
                source.voxel_count(),
                PADDED_VOXELS,
                "a surface brick is a padded cube"
            );
            source.write_values(values);
        }
        self.fill_gradients();
        let extent = self.grid.extent_of(coord);
        let cs = [
            extent.x() as usize,
            extent.y() as usize,
            extent.z() as usize,
        ];
        let origin = self.grid.origin(coord).map(|value| value as i64);
        let es = [cs[0] + 1, cs[1] + 1, cs[2] + 1];
        let count = es[0] * es[1] * es[2];
        self.mask.clear();
        self.mask.resize(count, false);
        self.labels.clear();
        self.labels.resize(count, EMPTY);
        let threshold = self.params.threshold();
        for rz in 0..es[2] {
            for ry in 0..es[1] {
                for rx in 0..es[0] {
                    let voxel = [
                        origin[0] + rx as i64,
                        origin[1] + ry as i64,
                        origin[2] + rz as i64,
                    ];
                    let inside = (0..3).all(|axis| {
                        voxel[axis] < i64::from(self.extent[axis])
                            && voxel[axis] >= i64::from(self.ranges[axis].start)
                            && voxel[axis] < i64::from(self.ranges[axis].end)
                    });
                    let index = rx + es[0] * (ry + es[1] * rz);
                    self.mask[index] = inside && self.value(rx, ry, rz) >= threshold;
                }
            }
        }
        self.label(&es, cs, origin);
        self.join(coord, cs, &es);
        self.store_faces(coord, cs, &es);
        self.emit(cs, &es, origin);
        self.next += 1;
    }

    pub fn finish(mut self) -> SurfaceSet {
        assert_eq!(
            self.next,
            self.coord_count(),
            "every surface brick has been added"
        );
        let mut objects = Vec::new();
        let mut intensities = Vec::new();
        let mut map = vec![EMPTY; self.nodes.len()];
        for (index, slot) in map.iter_mut().enumerate() {
            let node = self.nodes[index];
            if node.parent != index as u32 || node.triangles == 0 || node.voxels == 0 {
                continue;
            }
            let volume = node.voxels as f64 * self.voxel_volume;
            if volume < f64::from(self.params.min_volume()) || node.area <= 0.0 {
                continue;
            }
            let count = node.voxels as f64;
            let centroid = [0, 1, 2].map(|axis| {
                ((node.sum[axis] / count + 0.5) * f64::from(self.spacing[axis])
                    - 0.5 * f64::from(self.world[axis])) as f32
            });
            let low = [0, 1, 2].map(|axis| {
                (f64::from(node.low[axis]) * f64::from(self.spacing[axis])
                    - 0.5 * f64::from(self.world[axis])) as f32
            });
            let high = [0, 1, 2].map(|axis| {
                ((f64::from(node.high[axis]) + 1.0) * f64::from(self.spacing[axis])
                    - 0.5 * f64::from(self.world[axis])) as f32
            });
            *slot = objects.len() as u32;
            objects.push(SurfaceStatistics::new(
                node.voxels,
                volume,
                node.area,
                centroid,
                Bounds::new(low, high),
            ));
            for channel in 0..self.channels {
                intensities.push(self.accumulators[index * self.channels + channel].statistics());
            }
        }
        let mut vertex_map = vec![EMPTY; self.vertices.len()];
        let mut vertices = Vec::new();
        for (index, vertex) in self.vertices.iter().enumerate() {
            let root = find(&mut self.nodes, vertex.object());
            let object = map[root as usize];
            if object == EMPTY {
                continue;
            }
            vertex_map[index] = vertices.len() as u32;
            vertices.push(MeshVertex::new(vertex.position(), vertex.normal(), object));
        }
        let mut indices = Vec::new();
        for triangle in self.indices.as_chunks::<3>().0 {
            let mapped = [0, 1, 2].map(|corner| vertex_map[triangle[corner] as usize]);
            if mapped.iter().all(|index| *index != EMPTY) {
                indices.extend_from_slice(&mapped);
            }
        }
        SurfaceSet::new(
            self.level_index,
            Mesh::new(vertices, indices),
            objects,
            IntensityTable::new(self.channels, intensities),
        )
    }

    fn fill_gradients(&mut self) {
        for z in 0..PADDED {
            for y in 0..PADDED {
                for x in 0..PADDED {
                    let x0 = x.saturating_sub(1);
                    let x1 = (x + 1).min(PADDED - 1);
                    let y0 = y.saturating_sub(1);
                    let y1 = (y + 1).min(PADDED - 1);
                    let z0 = z.saturating_sub(1);
                    let z1 = (z + 1).min(PADDED - 1);
                    let gx = self.padded(x1, y, z) - self.padded(x0, y, z);
                    let gy = self.padded(x, y1, z) - self.padded(x, y0, z);
                    let gz = self.padded(x, y, z1) - self.padded(x, y, z0);
                    let gradient = [
                        gx / self.spacing[0],
                        gy / self.spacing[1],
                        gz / self.spacing[2],
                    ];
                    self.gradients[x + PADDED * (y + PADDED * z)] = gradient;
                }
            }
        }
    }

    fn label(&mut self, es: &[usize; 3], cs: [usize; 3], origin: [i64; 3]) {
        let strides = [1usize, es[0], es[0] * es[1]];
        let count = self.labels.len();
        let mut stack = Vec::new();
        for start in 0..count {
            if !self.mask[start] || self.labels[start] != EMPTY {
                continue;
            }
            let node = self.nodes.len() as u32;
            self.nodes.push(Node {
                parent: node,
                ..Node::new()
            });
            for _ in 0..self.channels {
                self.accumulators.push(SampleAccumulator::new());
            }
            self.labels[start] = node;
            stack.clear();
            stack.push(start);
            while let Some(cell) = stack.pop() {
                let rx = cell % es[0];
                let ry = (cell / es[0]) % es[1];
                let rz = cell / (es[0] * es[1]);
                let core = rx < cs[0] && ry < cs[1] && rz < cs[2];
                if core {
                    let voxel = [
                        (origin[0] + rx as i64) as u32,
                        (origin[1] + ry as i64) as u32,
                        (origin[2] + rz as i64) as u32,
                    ];
                    let stats = &mut self.nodes[node as usize];
                    stats.voxels += 1;
                    for (axis, coordinate) in voxel.iter().enumerate() {
                        stats.sum[axis] += f64::from(*coordinate);
                        stats.low[axis] = stats.low[axis].min(*coordinate);
                        stats.high[axis] = stats.high[axis].max(*coordinate);
                    }
                    let index = rx + 1 + PADDED * (ry + 1 + PADDED * (rz + 1));
                    for channel in 0..self.channels {
                        let value = self.values[channel][index];
                        self.accumulators[node as usize * self.channels + channel].push(value);
                    }
                }
                for axis in 0..3 {
                    let coordinate = [rx, ry, rz][axis];
                    for step in [-1i64, 1] {
                        let neighbour = cell as i64 + step * strides[axis] as i64;
                        let limit = if step < 0 {
                            coordinate > 0
                        } else {
                            coordinate + 1 < es[axis]
                        };
                        if !limit {
                            continue;
                        }
                        let neighbour = neighbour as usize;
                        if self.mask[neighbour] && self.labels[neighbour] == EMPTY {
                            self.labels[neighbour] = node;
                            stack.push(neighbour);
                        }
                    }
                }
            }
        }
    }

    fn join(&mut self, coord: BrickCoord, cs: [usize; 3], es: &[usize; 3]) {
        if coord.x() > self.first[0]
            && let Some(face) = self.x_face.take()
        {
            for ry in 0..cs[1] {
                for rz in 0..cs[2] {
                    let local = self.labels[ry * es[0] + rz * es[0] * es[1]];
                    let cached = face[ry + FACE_SIDE * rz];
                    if local != EMPTY && cached != EMPTY {
                        self.merge(local, cached);
                    }
                }
            }
        }
        if coord.y() > self.first[1]
            && let Some(face) = self.y_face[coord.x() as usize].take()
        {
            for rx in 0..cs[0] {
                for rz in 0..cs[2] {
                    let local = self.labels[rx + rz * es[0] * es[1]];
                    let cached = face[rx + FACE_SIDE * rz];
                    if local != EMPTY && cached != EMPTY {
                        self.merge(local, cached);
                    }
                }
            }
        }
        if coord.z() > self.first[2] {
            let index = coord.y() as usize * self.grid_counts[0] + coord.x() as usize;
            if let Some(face) = self.z_face[index].take() {
                for rx in 0..cs[0] {
                    for ry in 0..cs[1] {
                        let local = self.labels[rx + ry * es[0]];
                        let cached = face[rx + FACE_SIDE * ry];
                        if local != EMPTY && cached != EMPTY {
                            self.merge(local, cached);
                        }
                    }
                }
            }
        }
    }

    fn store_faces(&mut self, coord: BrickCoord, cs: [usize; 3], es: &[usize; 3]) {
        let mut face = vec![EMPTY; FACE_SIDE * FACE_SIDE];
        for ry in 0..=cs[1] {
            for rz in 0..=cs[2] {
                face[ry + FACE_SIDE * rz] = self.labels[cs[0] + es[0] * (ry + es[1] * rz)];
            }
        }
        self.x_face = Some(face);
        let mut face = vec![EMPTY; FACE_SIDE * FACE_SIDE];
        for rx in 0..=cs[0] {
            for rz in 0..=cs[2] {
                face[rx + FACE_SIDE * rz] = self.labels[rx + es[0] * (cs[1] + es[1] * rz)];
            }
        }
        self.y_face[coord.x() as usize] = Some(face);
        let mut face = vec![EMPTY; FACE_SIDE * FACE_SIDE];
        for rx in 0..=cs[0] {
            for ry in 0..=cs[1] {
                face[rx + FACE_SIDE * ry] = self.labels[rx + es[0] * ry + es[0] * es[1] * cs[2]];
            }
        }
        let index = coord.y() as usize * self.grid_counts[0] + coord.x() as usize;
        self.z_face[index] = Some(face);
    }

    fn emit(&mut self, cs: [usize; 3], es: &[usize; 3], origin: [i64; 3]) {
        let threshold = self.params.threshold();
        let low = [0, 1, 2].map(|axis| {
            (i64::from(self.ranges[axis].start) - origin[axis]).clamp(0, cs[axis] as i64) as usize
        });
        let high = [0, 1, 2].map(|axis| {
            (i64::from(self.ranges[axis].end) - origin[axis] - 1).clamp(0, cs[axis] as i64) as usize
        });
        self.edges.clear();
        let mut values = [0.0f32; 8];
        let mut corners = [[0usize; 3]; 8];
        for bz in low[2]..high[2] {
            for by in low[1]..high[1] {
                for bx in low[0]..high[0] {
                    let mut index = 0u32;
                    for (corner, offset) in CORNER.iter().enumerate() {
                        let rx = bx + offset[0];
                        let ry = by + offset[1];
                        let rz = bz + offset[2];
                        values[corner] = self.value(rx, ry, rz);
                        corners[corner] = [rx, ry, rz];
                        if values[corner] < threshold {
                            index |= 1 << corner;
                        }
                    }
                    if EDGE_TABLE[index as usize] == 0 {
                        continue;
                    }
                    let mut object = EMPTY;
                    for (corner, value) in values.iter().enumerate() {
                        if *value < threshold {
                            continue;
                        }
                        let [rx, ry, rz] = corners[corner];
                        let label = self.labels[rx + es[0] * (ry + es[1] * rz)];
                        if label != EMPTY {
                            object = label;
                            break;
                        }
                    }
                    if object == EMPTY {
                        continue;
                    }
                    let object = find(&mut self.nodes, object);
                    let table = TRIANGLE_TABLE[index as usize];
                    let mut corner = 0;
                    while corner < 16 && table[corner] >= 0 {
                        let mut triangle = [0u32; 3];
                        for (offset, vertex) in triangle.iter_mut().enumerate() {
                            *vertex = self.edge_vertex(
                                table[corner + offset] as usize,
                                &values,
                                &corners,
                                origin,
                                object,
                            );
                        }
                        self.push_triangle(triangle, object);
                        corner += 3;
                    }
                }
            }
        }
    }

    fn edge_vertex(
        &mut self,
        edge: usize,
        values: &[f32; 8],
        corners: &[[usize; 3]; 8],
        origin: [i64; 3],
        object: u32,
    ) -> u32 {
        let [a, b] = EDGE_CORNER[edge];
        let axis = EDGE_AXIS[edge];
        let lower = if corners[a][axis] < corners[b][axis] {
            a
        } else {
            b
        };
        let voxel = [
            origin[0] + corners[lower][0] as i64,
            origin[1] + corners[lower][1] as i64,
            origin[2] + corners[lower][2] as i64,
        ];
        let linear = voxel[0] as u64 * self.stride[0]
            + voxel[1] as u64 * self.stride[1]
            + voxel[2] as u64 * self.stride[2];
        let key = (linear << 2) | axis as u64;
        if let Some(vertex) = self.edges.get(&key) {
            return *vertex;
        }
        let low = values[a];
        let high = values[b];
        let threshold = self.params.threshold();
        let fraction = if (high - low).abs() > f32::EPSILON {
            ((threshold - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let point = [0, 1, 2].map(|component| {
            let low = corners[a][component] as f32;
            let high = corners[b][component] as f32;
            low + (high - low) * fraction
        });
        let normal = self.normal(point, low < high, axis);
        let world = [0, 1, 2].map(|component| {
            (origin[component] as f32 + point[component] + 0.5) * self.spacing[component]
                - 0.5 * self.world[component]
        });
        let vertex = self.vertices.len() as u32;
        self.vertices
            .push(MeshVertex::new(world, normal.to_array(), object));
        self.edges.insert(key, vertex);
        vertex
    }

    fn push_triangle(&mut self, triangle: [u32; 3], object: u32) {
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
        let stats = &mut self.nodes[object as usize];
        stats.area += f64::from(area);
        stats.triangles += 1;
    }

    fn merge(&mut self, left: u32, right: u32) -> u32 {
        let mut left = find(&mut self.nodes, left);
        let mut right = find(&mut self.nodes, right);
        if left == right {
            return left;
        }
        if self.nodes[left as usize].rank < self.nodes[right as usize].rank {
            std::mem::swap(&mut left, &mut right);
        }
        let winner = self.nodes[left as usize];
        let loser = self.nodes[right as usize];
        for channel in 0..self.channels {
            let other = self.accumulators[right as usize * self.channels + channel];
            self.accumulators[left as usize * self.channels + channel].merge(&other);
        }
        let target = &mut self.nodes[left as usize];
        target.voxels += loser.voxels;
        for axis in 0..3 {
            target.sum[axis] += loser.sum[axis];
            target.low[axis] = target.low[axis].min(loser.low[axis]);
            target.high[axis] = target.high[axis].max(loser.high[axis]);
        }
        target.area += loser.area;
        target.triangles += loser.triangles;
        if winner.rank == loser.rank {
            target.rank += 1;
        }
        self.nodes[right as usize].parent = left;
        left
    }

    fn normal(&self, point: [f32; 3], rising: bool, axis: usize) -> Vec3 {
        let gradient = self.gradient(point);
        if gradient.length_squared() > 1.0e-20 {
            return -gradient.normalize();
        }
        let mut direction = Vec3::ZERO;
        direction[axis] = if rising { -1.0 } else { 1.0 };
        direction
    }

    fn gradient(&self, point: [f32; 3]) -> Vec3 {
        let mut low = [0usize; 3];
        let mut high = [0usize; 3];
        let mut fraction = [0.0f32; 3];
        for axis in 0..3 {
            let padded = point[axis] + 1.0;
            let base = (padded.floor() as usize).min(PADDED - 1);
            low[axis] = base;
            high[axis] = (base + 1).min(PADDED - 1);
            fraction[axis] = (padded - base as f32).clamp(0.0, 1.0);
        }
        let mut gradient = Vec3::ZERO;
        for corner in 0..8 {
            let dx = corner & 1;
            let dy = (corner >> 1) & 1;
            let dz = (corner >> 2) & 1;
            let x = if dx == 0 { low[0] } else { high[0] };
            let y = if dy == 0 { low[1] } else { high[1] };
            let z = if dz == 0 { low[2] } else { high[2] };
            let weight = if dx == 0 {
                1.0 - fraction[0]
            } else {
                fraction[0]
            } * if dy == 0 {
                1.0 - fraction[1]
            } else {
                fraction[1]
            } * if dz == 0 {
                1.0 - fraction[2]
            } else {
                fraction[2]
            };
            gradient += Vec3::from(self.gradients[x + PADDED * (y + PADDED * z)]) * weight;
        }
        gradient
    }

    fn value(&self, rx: usize, ry: usize, rz: usize) -> f32 {
        self.padded(rx + 1, ry + 1, rz + 1)
    }

    fn padded(&self, x: usize, y: usize, z: usize) -> f32 {
        self.values[self.detection][x + PADDED * (y + PADDED * z)]
    }
}

fn find(nodes: &mut [Node], node: u32) -> u32 {
    let mut root = node;
    while nodes[root as usize].parent != root {
        root = nodes[root as usize].parent;
    }
    let mut current = node;
    while nodes[current as usize].parent != root {
        let next = nodes[current as usize].parent;
        nodes[current as usize].parent = root;
        current = next;
    }
    root
}
