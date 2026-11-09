use std::collections::HashMap;

use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex, SurfaceShape};
use grana_object::{IntensityTable, SampleAccumulator, SurfaceSet, SurfaceStatistics};
use grana_volume::{Level, VoxelData};

use crate::table::{CORNER, EDGE_CORNER, EDGE_TABLE, TRIANGLE_TABLE};

struct Object {
    label: u32,
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    edges: HashMap<u128, u32>,
    voxels: u64,
    sum: [f64; 3],
    low: [u32; 3],
    high: [u32; 3],
    accumulators: Vec<SampleAccumulator>,
}

pub struct LabelSlab<'a> {
    pub origin: [u32; 3],
    pub core: [u32; 3],
    pub extent: [u32; 3],
    pub labels: &'a [u32],
    pub channels: &'a [VoxelData],
}

pub struct LabelBuilder {
    level: usize,
    info: Level,
    channels: usize,
    objects: Vec<Object>,
    index: HashMap<u32, usize>,
}

impl LabelBuilder {
    pub fn new(level: usize, info: Level, channels: usize) -> Self {
        assert!(channels > 0, "a label image measures at least one channel");
        Self {
            level,
            info,
            channels,
            objects: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn push(&mut self, slab: LabelSlab<'_>) {
        let LabelSlab {
            origin,
            core,
            extent,
            labels,
            channels,
        } = slab;
        assert!(
            (0..3).all(|axis| core[axis] <= extent[axis]),
            "a label slab holds its core"
        );
        let volume = extent[0] as usize * extent[1] as usize * extent[2] as usize;
        assert_eq!(
            labels.len(),
            volume,
            "a label slab holds every voxel of its extent"
        );
        assert_eq!(
            channels.len(),
            self.channels,
            "a label slab measures every channel"
        );
        for channel in channels {
            assert_eq!(
                channel.voxel_count(),
                volume,
                "a channel slab covers the label slab"
            );
        }
        self.measure(origin, core, extent, labels, channels);
        self.geometry(origin, core, extent, labels);
    }

    pub fn finish(self) -> Option<SurfaceSet> {
        if self.objects.is_empty() {
            return None;
        }
        let mut objects = self.objects;
        objects.sort_by_key(|object| object.label);
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut statistics = Vec::with_capacity(objects.len());
        let mut intensities = Vec::with_capacity(objects.len() * self.channels);
        let voxel_volume = self.info.voxel_volume();
        for (object_index, object) in objects.iter().enumerate() {
            assert!(
                object.voxels > 0 && !object.indices.is_empty(),
                "a label object carries voxels and a closed surface"
            );
            let mut normals = vec![[0.0f32; 3]; object.positions.len()];
            let mut area = 0.0f64;
            for triangle in object.indices.as_chunks::<3>().0 {
                let points = triangle.map(|vertex| object.positions[vertex as usize]);
                let first = Vec3::from(points[0]);
                let cross = (Vec3::from(points[1]) - first).cross(Vec3::from(points[2]) - first);
                area += 0.5 * f64::from(cross.length());
                for vertex in triangle {
                    let normal = &mut normals[*vertex as usize];
                    for axis in 0..3 {
                        normal[axis] += cross[axis];
                    }
                }
            }
            let base = vertices.len() as u32;
            for (position, normal) in object.positions.iter().zip(&normals) {
                let normal = Vec3::from(*normal).normalize_or_zero();
                let normal = if normal == Vec3::ZERO {
                    [0.0, 0.0, 1.0]
                } else {
                    normal.to_array()
                };
                vertices.push(MeshVertex::new(*position, normal, object_index as u32));
            }
            indices.extend(object.indices.iter().map(|vertex| base + vertex));
            let count = object.voxels as f64;
            let centroid = self.info.voxel_center([
                (object.sum[0] / count) as f32,
                (object.sum[1] / count) as f32,
                (object.sum[2] / count) as f32,
            ]);
            let bounds = {
                let (low, high) = self
                    .info
                    .cell_bounds(object.low, object.high.map(|value| value + 1));
                Bounds::new(low, high)
            };
            let shape = SurfaceShape::of(&object.positions, object.indices.as_chunks::<3>().0);
            statistics.push(SurfaceStatistics::new(
                object.voxels,
                count * voxel_volume,
                area,
                centroid,
                bounds,
                shape,
            ));
            for accumulator in &object.accumulators {
                intensities.push(accumulator.statistics());
            }
        }
        Some(SurfaceSet::new(
            self.level,
            Mesh::new(vertices, indices),
            statistics,
            IntensityTable::new(self.channels, intensities),
        ))
    }

    fn measure(
        &mut self,
        origin: [u32; 3],
        core: [u32; 3],
        extent: [u32; 3],
        labels: &[u32],
        channels: &[VoxelData],
    ) {
        let width = extent[0] as usize;
        let height = extent[1] as usize;
        for z in 0..extent[2] {
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let slot = x as usize + width * (y as usize + height * z as usize);
                    let label = labels[slot];
                    if label == 0 {
                        continue;
                    }
                    let accumulators = self.channels;
                    let object = self.object(label, accumulators);
                    if x >= core[0] || y >= core[1] || z >= core[2] {
                        continue;
                    }
                    let cell = [origin[0] + x, origin[1] + y, origin[2] + z];
                    object.voxels += 1;
                    for (axis, value) in cell.iter().enumerate() {
                        object.sum[axis] += f64::from(*value);
                        object.low[axis] = object.low[axis].min(*value);
                        object.high[axis] = object.high[axis].max(*value);
                    }
                    for (channel, data) in channels.iter().enumerate() {
                        object.accumulators[channel].push(data.value(slot));
                    }
                }
            }
        }
    }

    fn geometry(&mut self, origin: [u32; 3], core: [u32; 3], extent: [u32; 3], labels: &[u32]) {
        for z in 0..core[2] {
            for y in 0..core[1] {
                for x in 0..core[0] {
                    let base = [origin[0] + x, origin[1] + y, origin[2] + z];
                    let mut corners = [0u32; 8];
                    let mut distinct = [0u32; 8];
                    let mut count = 0;
                    for corner in 0..8 {
                        let label = at(origin, extent, labels, base, &CORNER[corner]);
                        corners[corner] = label;
                        if label != 0 && !distinct[..count].contains(&label) {
                            distinct[count] = label;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        continue;
                    }
                    for label in &distinct[..count] {
                        let mut bits = 0u32;
                        for (corner, value) in corners.iter().enumerate() {
                            if value != label {
                                bits |= 1 << corner;
                            }
                        }
                        if EDGE_TABLE[bits as usize] == 0 {
                            continue;
                        }
                        let object = self.index[label];
                        let table = TRIANGLE_TABLE[bits as usize];
                        let mut entry = 0;
                        while entry < 16 && table[entry] >= 0 {
                            let mut triangle = [0u32; 3];
                            for (offset, vertex) in triangle.iter_mut().enumerate() {
                                let edge = EDGE_CORNER[table[entry + offset] as usize];
                                *vertex = self.vertex(object, base, edge);
                            }
                            if triangle[0] != triangle[1]
                                && triangle[1] != triangle[2]
                                && triangle[2] != triangle[0]
                            {
                                let target = &mut self.objects[object].indices;
                                target.extend_from_slice(&triangle);
                            }
                            entry += 3;
                        }
                    }
                }
            }
        }
    }

    fn vertex(&mut self, object: usize, base: [u32; 3], edge: [usize; 2]) -> u32 {
        let first = CORNER[edge[0]];
        let second = CORNER[edge[1]];
        let mut lower = [0u32; 3];
        let mut axis = 0;
        for component in 0..3 {
            lower[component] = base[component] + first[component].min(second[component]) as u32;
            if first[component] != second[component] {
                axis = component;
            }
        }
        let key = (u128::from(lower[0]) << 66)
            | (u128::from(lower[1]) << 34)
            | (u128::from(lower[2]) << 2)
            | axis as u128;
        let target = &mut self.objects[object];
        if let Some(vertex) = target.edges.get(&key) {
            return *vertex;
        }
        let mut point = [0.0f32; 3];
        for component in 0..3 {
            let offset = if component == axis { 0.5 } else { 0.0 };
            point[component] = lower[component] as f32 + offset;
        }
        let world = self.info.voxel_center(point);
        let vertex = target.positions.len() as u32;
        target.positions.push(world);
        target.edges.insert(key, vertex);
        vertex
    }

    fn object(&mut self, label: u32, channels: usize) -> &mut Object {
        let slot = match self.index.get(&label) {
            Some(slot) => *slot,
            None => {
                let slot = self.objects.len();
                self.index.insert(label, slot);
                self.objects.push(Object {
                    label,
                    positions: Vec::new(),
                    indices: Vec::new(),
                    edges: HashMap::new(),
                    voxels: 0,
                    sum: [0.0; 3],
                    low: [u32::MAX; 3],
                    high: [0; 3],
                    accumulators: vec![SampleAccumulator::new(); channels],
                });
                slot
            }
        };
        &mut self.objects[slot]
    }
}

fn at(
    origin: [u32; 3],
    extent: [u32; 3],
    labels: &[u32],
    base: [u32; 3],
    corner: &[i64; 3],
) -> u32 {
    let mut local = [0usize; 3];
    for axis in 0..3 {
        let value = i64::from(base[axis]) + corner[axis] - i64::from(origin[axis]);
        if value < 0 || value >= i64::from(extent[axis]) {
            return 0;
        }
        local[axis] = value as usize;
    }
    labels[local[0] + extent[0] as usize * (local[1] + extent[1] as usize * local[2])]
}
