use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{IntensityTable, SampleAccumulator, SurfaceSet, SurfaceStatistics};
use grana_volume::{Intensity, Level, Region, VoxelData};

use crate::SurfaceParams;
use crate::mask::Mask;
use crate::table::{EDGE_TABLE, TRIANGLE_TABLE};

const TILE_SIDE: u32 = 96;
const DISTANCE_LIMIT: u32 = 12;
const GRAIN_LIMIT: f32 = 3.0;
const GRAIN_SUPPORT: f32 = 3.0;
const ANISOTROPY_LIMIT: u32 = 16;
const EMPTY: u32 = u32::MAX;

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

#[derive(Clone, Copy)]
struct Node {
    parent: u32,
    rank: u8,
    reach: u32,
    voxels: u64,
    sum: [f64; 3],
    low: [u32; 3],
    high: [u32; 3],
    area: f64,
    triangles: u64,
}

impl Node {
    fn new(reach: u32) -> Self {
        Self {
            parent: 0,
            rank: 0,
            reach,
            voxels: 0,
            sum: [0.0; 3],
            low: [u32::MAX; 3],
            high: [0; 3],
            area: 0.0,
            triangles: 0,
        }
    }
}

struct Face {
    origin: [u32; 2],
    dims: [u32; 2],
    labels: Vec<u32>,
}

pub struct SurfaceBuilder {
    level_index: usize,
    detection: usize,
    channels: usize,
    extent: [u32; 3],
    spacing: [f32; 3],
    world: [f32; 3],
    voxel_volume: f64,
    params: SurfaceParams,
    threshold: f32,
    region: [Range<u32>; 3],
    tiles: [u32; 3],
    tile_count: usize,
    next: usize,
    split: bool,
    merge_gap: u32,
    anisotropy: [u32; 3],
    distance_cap: u16,
    kernels: [Vec<f32>; 3],
    pad: [u32; 3],
    dims: [usize; 3],
    field: Vec<f32>,
    mask: Vec<bool>,
    distance: Vec<u16>,
    labels: Vec<u32>,
    order: Vec<u32>,
    stack: Vec<u32>,
    nodes: Vec<Node>,
    accumulators: Vec<SampleAccumulator>,
    necks: Vec<(u32, u32, u16)>,
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
    edges: HashMap<(u64, u32), u32>,
    x_face: Option<Face>,
    y_faces: Vec<Option<Face>>,
    z_faces: Vec<Option<Face>>,
    stride: [u64; 3],
}

impl SurfaceBuilder {
    pub fn new(
        level_index: usize,
        level: Level,
        params: SurfaceParams,
        channels: usize,
        detection: usize,
        region: Region,
        intensity: Intensity,
    ) -> Self {
        assert!(
            channels > 0 && detection < channels,
            "the detection channel is one of the measured channels"
        );
        let threshold = intensity.value(params.threshold());
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let world = level.world_extent();
        let voxel_volume = f64::from(spacing[0]) * f64::from(spacing[1]) * f64::from(spacing[2]);
        let region = region.voxel_range(level);
        let region = region.unwrap_or([0..0, 0..0, 0..0]);
        let narrowest = spacing.iter().copied().fold(f32::INFINITY, f32::min);
        let grain = params.grain().min(GRAIN_LIMIT * narrowest);
        let anisotropy =
            spacing.map(|value| ((value / narrowest).round() as u32).clamp(1, ANISOTROPY_LIMIT));
        let split = params.seed() > 0.0;
        let reach = if split { DISTANCE_LIMIT } else { 0 };
        let widest = anisotropy.iter().copied().max().expect("three axes exist");
        let distance_cap = ((reach * widest) * (reach * widest)) as u16;
        let merge_gap = (params.seed() / (2.0 * narrowest)).round().max(1.0) as u32;
        let kernels = spacing.map(|value| gaussian((grain / value).min(GRAIN_LIMIT)));
        let pad = [0, 1, 2].map(|axis| reach + (kernels[axis].len() / 2) as u32 + 1);
        let tiles = [0, 1, 2].map(|axis| {
            let count = region[axis].end.saturating_sub(region[axis].start);
            count.div_ceil(TILE_SIDE)
        });
        let tile_count = tiles[0] as usize * tiles[1] as usize * tiles[2] as usize;
        let stride = [
            1,
            u64::from(extent[0]),
            u64::from(extent[0]) * u64::from(extent[1]),
        ];
        let y_faces = (0..tiles[0]).map(|_| None).collect();
        let z_faces = (0..tiles[0] * tiles[1]).map(|_| None).collect();
        Self {
            level_index,
            detection,
            channels,
            extent,
            spacing,
            world,
            voxel_volume,
            params,
            threshold,
            region,
            tiles,
            tile_count,
            next: 0,
            split,
            merge_gap,
            anisotropy,
            distance_cap,
            kernels,
            pad,
            dims: [0; 3],
            field: Vec::new(),
            mask: Vec::new(),
            distance: Vec::new(),
            labels: Vec::new(),
            order: Vec::new(),
            stack: Vec::new(),
            nodes: Vec::new(),
            accumulators: Vec::new(),
            necks: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            edges: HashMap::new(),
            x_face: None,
            y_faces,
            z_faces,
            stride,
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tile_count
    }

    pub fn core_range(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.tile_count, "the tile index is in bounds");
        let [x, y, z] = self.tile_position(index);
        [0, 1, 2].map(|axis| {
            let start = self.region[axis].start + [x, y, z][axis] * TILE_SIDE;
            start..(start + TILE_SIDE).min(self.region[axis].end)
        })
    }

    pub fn detection_range(&self, index: usize) -> [Range<u32>; 3] {
        let core = self.core_range(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(self.pad[axis]);
            start..(core[axis].end + self.pad[axis]).min(self.extent[axis])
        })
    }

    pub fn add(
        &mut self,
        index: usize,
        detection: &VoxelData,
        roi: Option<&Mask>,
        mut measure: impl FnMut(usize, &[Range<u32>; 3]) -> Result<VoxelData, String>,
    ) -> Result<(), String> {
        assert_eq!(index, self.next, "surface tiles arrive in raster order");
        let core = self.core_range(index);
        let array = self.detection_range(index);
        self.dims = dims_of(&array);
        let count = self.dims[0] * self.dims[1] * self.dims[2];
        if detection.voxel_count() != count {
            return Err("the detection tile does not cover the requested region".to_string());
        }
        self.field.clear();
        self.field.resize(count, 0.0);
        detection.write_values(&mut self.field);
        self.smooth();
        self.threshold_mask(roi);
        self.labels.clear();
        self.labels.resize(count, EMPTY);
        if self.split {
            self.watershed();
            self.record_necks();
        } else {
            self.flood();
        }
        self.join();
        self.accumulate(&core, &array, &mut measure)?;
        self.store_faces(&core, &array);
        self.emit();
        self.next += 1;
        Ok(())
    }

    pub fn finish(mut self) -> SurfaceSet {
        assert_eq!(
            self.next, self.tile_count,
            "every surface tile has been added"
        );
        self.merge_basins();
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

    fn tile_position(&self, index: usize) -> [u32; 3] {
        let x = index % self.tiles[0] as usize;
        let y = (index / self.tiles[0] as usize) % self.tiles[1] as usize;
        let z = index / (self.tiles[0] as usize * self.tiles[1] as usize);
        [x as u32, y as u32, z as u32]
    }

    fn array_range(&self) -> [Range<u32>; 3] {
        self.detection_range(self.next)
    }

    fn smooth(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        for axis in 0..3 {
            let kernel = &self.kernels[axis];
            if kernel.len() <= 1 {
                continue;
            }
            let radius = (kernel.len() / 2) as i64;
            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
            let length = dims[axis];
            let mut line = vec![0.0f32; length];
            let mut blurred = vec![0.0f32; length];
            for position in 0..dims[second] {
                for row in 0..dims[first] {
                    let base = strides[first] * row + strides[second] * position;
                    for (step, slot) in line.iter_mut().enumerate() {
                        *slot = self.field[base + strides[axis] * step];
                    }
                    for (step, slot) in blurred.iter_mut().enumerate() {
                        let mut sum = 0.0;
                        for (tap, weight) in kernel.iter().enumerate() {
                            let sample = (step as i64 + tap as i64 - radius)
                                .clamp(0, length as i64 - 1)
                                as usize;
                            sum += line[sample] * weight;
                        }
                        *slot = sum;
                    }
                    for (step, value) in blurred.iter().enumerate() {
                        self.field[base + strides[axis] * step] = *value;
                    }
                }
            }
        }
    }

    fn threshold_mask(&mut self, roi: Option<&Mask>) {
        let dims = self.dims;
        let array = self.array_range();
        let count = dims[0] * dims[1] * dims[2];
        self.mask.clear();
        self.mask.resize(count, false);
        let threshold = self.threshold;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let index = x + dims[0] * (y + dims[1] * z);
                    if self.field[index] < threshold {
                        continue;
                    }
                    let voxel = [
                        array[0].start + x as u32,
                        array[1].start + y as u32,
                        array[2].start + z as u32,
                    ];
                    if (0..3).any(|axis| {
                        voxel[axis] < self.region[axis].start
                            || voxel[axis] >= self.region[axis].end
                    }) {
                        continue;
                    }
                    if roi.is_some_and(|roi| {
                        !roi.contains([0, 1, 2].map(|axis| {
                            (voxel[axis] as f32 + 0.5) * self.spacing[axis] - 0.5 * self.world[axis]
                        }))
                    }) {
                        continue;
                    }
                    self.mask[index] = true;
                }
            }
        }
    }

    fn flood(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        for start in 0..count {
            if !self.mask[start] || self.labels[start] != EMPTY {
                continue;
            }
            let node = self.create_node(0);
            self.labels[start] = node;
            self.stack.clear();
            self.stack.push(start as u32);
            while let Some(cell) = self.stack.pop() {
                let cell = cell as usize;
                for axis in 0..3 {
                    let coordinate = (cell / strides[axis]) % dims[axis];
                    for step in [-1i64, 1] {
                        let reached = coordinate as i64 + step;
                        if reached < 0 || reached >= dims[axis] as i64 {
                            continue;
                        }
                        let neighbour = (cell as i64 + step * strides[axis] as i64) as usize;
                        if self.mask[neighbour] && self.labels[neighbour] == EMPTY {
                            self.labels[neighbour] = node;
                            self.stack.push(neighbour as u32);
                        }
                    }
                }
            }
        }
    }

    fn watershed(&mut self) {
        self.fill_distance();
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.distance_cap as usize;
        let mut counts = vec![0u32; cap + 1];
        for index in 0..count {
            if self.mask[index] {
                counts[self.distance[index] as usize] += 1;
            }
        }
        let mut cursor = vec![0u32; cap + 1];
        let mut total = 0u32;
        for bucket in (0..=cap).rev() {
            cursor[bucket] = total;
            total += counts[bucket];
        }
        self.order.clear();
        self.order.resize(total as usize, 0);
        for index in 0..count {
            if !self.mask[index] {
                continue;
            }
            let bucket = self.distance[index] as usize;
            self.order[cursor[bucket] as usize] = index as u32;
            cursor[bucket] += 1;
        }
        for position in 0..self.order.len() {
            let index = self.order[position] as usize;
            let level = self.distance[index];
            let mut best = EMPTY;
            for axis in 0..3 {
                let coordinate = (index / strides[axis]) % dims[axis];
                for step in [-1i64, 1] {
                    let reached = coordinate as i64 + step;
                    if reached < 0 || reached >= dims[axis] as i64 {
                        continue;
                    }
                    let neighbour = (index as i64 + step * strides[axis] as i64) as usize;
                    if !self.mask[neighbour] {
                        continue;
                    }
                    let compete = best == EMPTY
                        || self.distance[neighbour] > self.distance[best as usize]
                        || (self.distance[neighbour] == self.distance[best as usize]
                            && neighbour < best as usize);
                    if compete {
                        best = neighbour as u32;
                    }
                }
            }
            if best != EMPTY
                && (self.distance[best as usize] > level
                    || (self.distance[best as usize] == level && (best as usize) < index))
            {
                assert!(
                    self.labels[best as usize] != EMPTY,
                    "the successor of a watershed voxel is labelled"
                );
                self.labels[index] = self.labels[best as usize];
            } else {
                self.labels[index] = self.create_node(u32::from(level));
            }
        }
    }

    fn fill_distance(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.distance_cap;
        self.distance.clear();
        self.distance.resize(count, 0);
        for index in 0..count {
            self.distance[index] = if self.mask[index] { cap } else { 0 };
        }
        let anisotropy = self.anisotropy;
        for axis in 0..3 {
            let weight = f64::from(anisotropy[axis] * anisotropy[axis]);
            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
            let length = dims[axis];
            let mut line = vec![0.0f64; length];
            for position in 0..dims[second] {
                for row in 0..dims[first] {
                    let base = strides[first] * row + strides[second] * position;
                    for (step, slot) in line.iter_mut().enumerate() {
                        *slot = f64::from(self.distance[base + strides[axis] * step]);
                    }
                    parabola(&mut line, weight);
                    for (step, value) in line.iter().enumerate() {
                        self.distance[base + strides[axis] * step] =
                            value.min(f64::from(cap)) as u16;
                    }
                }
            }
        }
    }

    fn record_necks(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let mut pairs: HashMap<(u32, u32), u16> = HashMap::new();
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let index = x + dims[0] * (y + dims[1] * z);
                    if !self.mask[index] {
                        continue;
                    }
                    for axis in 0..3 {
                        if [x, y, z][axis] + 1 >= dims[axis] {
                            continue;
                        }
                        let neighbour = index + strides[axis];
                        if !self.mask[neighbour] {
                            continue;
                        }
                        let left = find(&mut self.nodes, self.labels[index]);
                        let right = find(&mut self.nodes, self.labels[neighbour]);
                        if left == right {
                            continue;
                        }
                        let key = (left.min(right), left.max(right));
                        let neck = self.distance[index].min(self.distance[neighbour]);
                        let slot = pairs.entry(key).or_insert(0);
                        *slot = (*slot).max(neck);
                    }
                }
            }
        }
        let mut keys = pairs.keys().copied().collect::<Vec<_>>();
        keys.sort_unstable();
        for key in keys {
            self.necks.push((key.0, key.1, pairs[&key]));
        }
    }

    fn accumulate(
        &mut self,
        core: &[Range<u32>; 3],
        array: &[Range<u32>; 3],
        measure: &mut impl FnMut(usize, &[Range<u32>; 3]) -> Result<VoxelData, String>,
    ) -> Result<(), String> {
        let dims = self.dims;
        let core_dims = dims_of(core);
        let core_count = core_dims[0] * core_dims[1] * core_dims[2];
        let origin = [
            (core[0].start - array[0].start) as usize,
            (core[1].start - array[1].start) as usize,
            (core[2].start - array[2].start) as usize,
        ];
        let mut measured = Vec::with_capacity(self.channels);
        for channel in 0..self.channels {
            if channel == self.detection {
                measured.push(None);
                continue;
            }
            let data = measure(channel, core)?;
            if data.voxel_count() != core_count {
                return Err("the measured tile does not cover the requested region".to_string());
            }
            measured.push(Some(data));
        }
        for z in 0..core_dims[2] {
            for y in 0..core_dims[1] {
                for x in 0..core_dims[0] {
                    let index =
                        (origin[0] + x) + dims[0] * ((origin[1] + y) + dims[1] * (origin[2] + z));
                    if !self.mask[index] {
                        continue;
                    }
                    let node = find(&mut self.nodes, self.labels[index]) as usize;
                    let local = x + core_dims[0] * (y + core_dims[1] * z);
                    for (channel, data) in measured.iter().enumerate() {
                        let value = match data {
                            Some(data) => data.value(local),
                            None => self.field[index],
                        };
                        self.accumulators[node * self.channels + channel].push(value);
                    }
                    let voxel = [
                        core[0].start + x as u32,
                        core[1].start + y as u32,
                        core[2].start + z as u32,
                    ];
                    let stats = &mut self.nodes[node];
                    stats.voxels += 1;
                    for (axis, coordinate) in voxel.iter().enumerate() {
                        stats.sum[axis] += f64::from(*coordinate);
                        stats.low[axis] = stats.low[axis].min(*coordinate);
                        stats.high[axis] = stats.high[axis].max(*coordinate);
                    }
                }
            }
        }
        Ok(())
    }

    fn join(&mut self) {
        let [x, y, z] = self.tile_position(self.next);
        let strides = strides_of(self.dims);
        if x > 0
            && let Some(face) = self.x_face.take()
        {
            let plane = self.join_plane(0);
            self.join_face(&face, 0, plane, strides);
        }
        if y > 0
            && let Some(face) = self.y_faces[x as usize].take()
        {
            let plane = self.join_plane(1);
            self.join_face(&face, 1, plane, strides);
        }
        if z > 0 {
            let slot = y as usize * self.tiles[0] as usize + x as usize;
            if let Some(face) = self.z_faces[slot].take() {
                let plane = self.join_plane(2);
                self.join_face(&face, 2, plane, strides);
            }
        }
    }

    fn join_plane(&self, axis: usize) -> usize {
        let core = self.core_range(self.next);
        let array = self.array_range();
        (core[axis].start - array[axis].start) as usize
    }

    fn join_face(&mut self, face: &Face, axis: usize, plane: usize, strides: [usize; 3]) {
        let array = self.array_range();
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        let start = [
            face.origin[0].max(array[first].start),
            face.origin[1].max(array[second].start),
        ];
        let end = [
            (face.origin[0] + face.dims[0]).min(array[first].end),
            (face.origin[1] + face.dims[1]).min(array[second].end),
        ];
        if start[0] >= end[0] || start[1] >= end[1] {
            return;
        }
        for second_position in start[1]..end[1] {
            for first_position in start[0]..end[0] {
                let cached = face.labels[(first_position - face.origin[0]) as usize
                    + face.dims[0] as usize * (second_position - face.origin[1]) as usize];
                if cached == EMPTY {
                    continue;
                }
                let index = plane
                    + strides[first] * (first_position - array[first].start) as usize
                    + strides[second] * (second_position - array[second].start) as usize;
                let local = self.labels[index];
                if local == EMPTY {
                    continue;
                }
                self.union(local, cached);
            }
        }
    }

    fn store_faces(&mut self, core: &[Range<u32>; 3], array: &[Range<u32>; 3]) {
        let [x, y, _] = self.tile_position(self.next);
        if core[0].end < array[0].end {
            self.x_face = Some(self.slice(0, (core[0].end - array[0].start) as usize));
        }
        if core[1].end < array[1].end {
            self.y_faces[x as usize] = Some(self.slice(1, (core[1].end - array[1].start) as usize));
        }
        if core[2].end < array[2].end {
            let slot = y as usize * self.tiles[0] as usize + x as usize;
            self.z_faces[slot] = Some(self.slice(2, (core[2].end - array[2].start) as usize));
        }
    }

    fn slice(&self, axis: usize, plane: usize) -> Face {
        let dims = self.dims;
        let strides = strides_of(dims);
        let array = self.array_range();
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        let mut labels = Vec::with_capacity(dims[first] * dims[second]);
        for second_position in 0..dims[second] {
            for first_position in 0..dims[first] {
                labels.push(
                    self.labels[plane
                        + strides[first] * first_position
                        + strides[second] * second_position],
                );
            }
        }
        Face {
            origin: [array[first].start, array[second].start],
            dims: [dims[first] as u32, dims[second] as u32],
            labels,
        }
    }

    fn emit(&mut self) {
        let dims = self.dims;
        let core = self.core_range(self.next);
        let array = self.array_range();
        let threshold = self.threshold;
        let low = [0, 1, 2].map(|axis| (core[axis].start - array[axis].start) as usize);
        let high = [0, 1, 2].map(|axis| {
            (core[axis].end.min(self.region[axis].end - 1) - array[axis].start) as usize
        });
        self.edges.clear();
        let mut values = [0.0f32; 8];
        let mut corners = [[0usize; 3]; 8];
        for bz in low[2]..high[2] {
            for by in low[1]..high[1] {
                for bx in low[0]..high[0] {
                    let mut index = 0u32;
                    for (corner, offset) in CORNER.iter().enumerate() {
                        let cell = [bx + offset[0], by + offset[1], bz + offset[2]];
                        values[corner] =
                            self.field[cell[0] + dims[0] * (cell[1] + dims[1] * cell[2])];
                        corners[corner] = cell;
                        if values[corner] < threshold {
                            index |= 1 << corner;
                        }
                    }
                    if EDGE_TABLE[index as usize] == 0 {
                        continue;
                    }
                    let mut object = EMPTY;
                    let mut deepest = threshold;
                    for (corner, value) in values.iter().enumerate() {
                        if *value < deepest {
                            continue;
                        }
                        let [x, y, z] = corners[corner];
                        let label = self.labels[x + dims[0] * (y + dims[1] * z)];
                        if label == EMPTY {
                            continue;
                        }
                        object = find(&mut self.nodes, label);
                        deepest = *value;
                    }
                    if object == EMPTY {
                        continue;
                    }
                    let table = TRIANGLE_TABLE[index as usize];
                    let mut corner = 0;
                    while corner < 16 && table[corner] >= 0 {
                        let mut triangle = [0u32; 3];
                        for (offset, vertex) in triangle.iter_mut().enumerate() {
                            *vertex = self.edge_vertex(
                                table[corner + offset] as usize,
                                &values,
                                &corners,
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
        object: u32,
    ) -> u32 {
        let [a, b] = EDGE_CORNER[edge];
        let axis = EDGE_AXIS[edge];
        let lower = if corners[a][axis] < corners[b][axis] {
            a
        } else {
            b
        };
        let array = self.array_range();
        let voxel = [
            array[0].start + corners[lower][0] as u32,
            array[1].start + corners[lower][1] as u32,
            array[2].start + corners[lower][2] as u32,
        ];
        let linear = u64::from(voxel[0]) * self.stride[0]
            + u64::from(voxel[1]) * self.stride[1]
            + u64::from(voxel[2]) * self.stride[2];
        let key = ((linear << 2) | axis as u64, object);
        if let Some(vertex) = self.edges.get(&key) {
            return *vertex;
        }
        let low = values[a];
        let high = values[b];
        let threshold = self.threshold;
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
            let voxel = array[component].start as f32 + point[component];
            (voxel + 0.5) * self.spacing[component] - 0.5 * self.world[component]
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
        let node = find(&mut self.nodes, object);
        let stats = &mut self.nodes[node as usize];
        stats.area += f64::from(area);
        stats.triangles += 1;
    }

    fn normal(&self, point: [f32; 3], rising: bool, axis: usize) -> Vec3 {
        let gradient = Vec3::new(
            (self.sample([point[0] + 1.0, point[1], point[2]])
                - self.sample([point[0] - 1.0, point[1], point[2]]))
                / (2.0 * self.spacing[0]),
            (self.sample([point[0], point[1] + 1.0, point[2]])
                - self.sample([point[0], point[1] - 1.0, point[2]]))
                / (2.0 * self.spacing[1]),
            (self.sample([point[0], point[1], point[2] + 1.0])
                - self.sample([point[0], point[1], point[2] - 1.0]))
                / (2.0 * self.spacing[2]),
        );
        if gradient.length_squared() > 1.0e-20 {
            return -gradient.normalize();
        }
        let mut direction = Vec3::ZERO;
        direction[axis] = if rising { -1.0 } else { 1.0 };
        direction
    }

    fn sample(&self, point: [f32; 3]) -> f32 {
        let dims = self.dims;
        let mut low = [0usize; 3];
        let mut high = [0usize; 3];
        let mut fraction = [0.0f32; 3];
        for axis in 0..3 {
            let value = point[axis].clamp(0.0, (dims[axis] - 1) as f32);
            let base = value.floor() as usize;
            low[axis] = base;
            high[axis] = (base + 1).min(dims[axis] - 1);
            fraction[axis] = (value - base as f32).clamp(0.0, 1.0);
        }
        let mut total = 0.0;
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
            total += self.field[x + dims[0] * (y + dims[1] * z)] * weight;
        }
        total
    }

    fn create_node(&mut self, reach: u32) -> u32 {
        let node = self.nodes.len() as u32;
        self.nodes.push(Node {
            parent: node,
            ..Node::new(reach)
        });
        for _ in 0..self.channels {
            self.accumulators.push(SampleAccumulator::new());
        }
        node
    }

    fn union(&mut self, left: u32, right: u32) -> u32 {
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
        target.reach = target.reach.max(loser.reach);
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

    fn merge_basins(&mut self) {
        let mut seams: HashMap<(u32, u32), u16> = HashMap::new();
        for (left, right, neck) in std::mem::take(&mut self.necks) {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let key = (left.min(right), left.max(right));
            let slot = seams.entry(key).or_insert(0);
            *slot = (*slot).max(neck);
        }
        let mut seams = seams.into_iter().collect::<Vec<_>>();
        seams.sort_unstable_by(|(left_key, left_neck), (right_key, right_neck)| {
            right_neck.cmp(left_neck).then(left_key.cmp(right_key))
        });
        let gap = self.merge_gap;
        for ((left, right), neck) in seams {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let reach = length(self.nodes[left as usize].reach)
                .min(length(self.nodes[right as usize].reach));
            let drop = i64::from(reach) - i64::from(length(u32::from(neck)));
            if drop < i64::from(gap) {
                self.union(left, right);
            }
        }
    }
}

fn gaussian(sigma: f32) -> Vec<f32> {
    let support = (GRAIN_SUPPORT * sigma).ceil() as usize;
    if support == 0 {
        return vec![1.0];
    }
    let mut kernel = (0..=2 * support)
        .map(|index| {
            let offset = index as f32 - support as f32;
            (-0.5 * (offset / sigma) * (offset / sigma)).exp()
        })
        .collect::<Vec<_>>();
    let total: f32 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    kernel
}

fn parabola(line: &mut [f64], weight: f64) {
    let length = line.len();
    if length < 3 {
        return;
    }
    let mut locations = vec![0usize; length];
    let mut boundaries = vec![0.0f64; length + 1];
    let mut envelope = 0usize;
    boundaries[0] = f64::NEG_INFINITY;
    boundaries[1] = f64::INFINITY;
    for position in 1..length {
        let mut boundary = crossing(line, &locations, envelope, position, weight);
        while envelope > 0 && boundary <= boundaries[envelope] {
            envelope -= 1;
            boundary = crossing(line, &locations, envelope, position, weight);
        }
        envelope += 1;
        locations[envelope] = position;
        boundaries[envelope] = boundary;
        boundaries[envelope + 1] = f64::INFINITY;
    }
    let mut envelope = 0usize;
    for position in 0..length {
        while boundaries[envelope + 1] < position as f64 {
            envelope += 1;
        }
        let distance = position as f64 - locations[envelope] as f64;
        line[position] = weight * distance * distance + line[locations[envelope]];
    }
}

fn crossing(
    line: &[f64],
    locations: &[usize],
    envelope: usize,
    position: usize,
    weight: f64,
) -> f64 {
    let anchor = locations[envelope];
    ((line[position] + weight * (position * position) as f64)
        - (line[anchor] + weight * (anchor * anchor) as f64))
        / (2.0 * weight * (position as f64 - anchor as f64))
}

fn dims_of(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [0, 1, 2].map(|axis| (ranges[axis].end - ranges[axis].start) as usize)
}

fn length(squared: u32) -> u32 {
    (squared as f64).sqrt() as u32
}

fn strides_of(dims: [usize; 3]) -> [usize; 3] {
    [1, dims[0], dims[0] * dims[1]]
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
