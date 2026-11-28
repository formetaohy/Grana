use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_geometry::marching::{CORNER, EDGE_CORNER, EDGE_TABLE, TRIANGLE_TABLE};
use grana_geometry::{Mesh, MeshVertex};
use grana_volume::Level;

const HALO: u32 = 1;
const WORD: usize = 64;
const BACKGROUND: u32 = 0;

pub struct TraceBand<'a> {
    level: Level,
    origin: [u32; 3],
    extent: [u32; 3],
    cells: [Range<u32>; 3],
    labels: &'a [u32],
}

impl<'a> TraceBand<'a> {
    pub fn new(
        level: Level,
        origin: [u32; 3],
        extent: [u32; 3],
        cells: [Range<u32>; 3],
        labels: &'a [u32],
    ) -> Self {
        assert_eq!(
            labels.len(),
            extent
                .iter()
                .map(|value| *value as usize)
                .product::<usize>(),
            "a label band holds every voxel of its extent"
        );
        for axis in 0..3 {
            assert!(
                cells[axis].start < cells[axis].end && cells[axis].end < extent[axis],
                "a label band emits cells whose corners it holds"
            );
        }
        Self {
            level,
            origin,
            extent,
            cells,
            labels,
        }
    }
}

pub struct Traced {
    pub mesh: Mesh,
    pub labels: Vec<u32>,
}

#[derive(Default)]
pub struct Tracer {
    labels: Vec<u32>,
    slots: HashMap<u32, usize>,
    vertices: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    objects: Vec<u32>,
    edges: HashMap<u128, u32>,
    indices: Vec<u32>,
    volume: Vec<f64>,
}

impl Tracer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn push(&mut self, band: TraceBand<'_>) {
        let TraceBand {
            level,
            origin,
            extent,
            cells,
            labels,
        } = band;
        let dims = [0, 1, 2].map(|axis| extent[axis] as usize);
        let stride = [1usize, dims[0], dims[0] * dims[1]];
        let scan = [0, 1, 2].map(|axis| {
            cells[axis].start.saturating_sub(HALO)..(cells[axis].end + HALO).min(extent[axis] - 1)
        });
        let span = [0, 1, 2].map(|axis| (scan[axis].end - scan[axis].start) as usize);
        let words = span[0].div_ceil(WORD);
        let mut active = vec![0u64; words * span[1] * span[2]];
        let voxels = [0, 1, 2]
            .map(|axis| scan[axis].start.saturating_sub(1)..(scan[axis].end + 1).min(extent[axis]));
        for z in voxels[2].clone() {
            for y in voxels[1].clone() {
                for x in voxels[0].clone() {
                    let voxel =
                        x as usize * stride[0] + y as usize * stride[1] + z as usize * stride[2];
                    if labels[voxel] == BACKGROUND {
                        continue;
                    }
                    for dz in z.saturating_sub(1)..=(z + 1).min(scan[2].end - 1) {
                        for dy in y.saturating_sub(1)..=(y + 1).min(scan[1].end - 1) {
                            let row = (dz - scan[2].start) as usize * span[1]
                                + (dy - scan[1].start) as usize;
                            for dx in x.saturating_sub(1)..=(x + 1).min(scan[0].end - 1) {
                                let column = dx - scan[0].start;
                                active[row * words + column as usize / WORD] |=
                                    1u64 << (column as usize % WORD);
                            }
                        }
                    }
                }
            }
        }
        for z in scan[2].clone() {
            for y in scan[1].clone() {
                let row = (z - scan[2].start) as usize * span[1] + (y - scan[1].start) as usize;
                for word in 0..words {
                    let mut bits = active[row * words + word];
                    while bits != 0 {
                        let column = word * WORD + bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        let cell = [scan[0].start + column as u32, y, z];
                        let emit = (0..3).all(|axis| cells[axis].contains(&cell[axis]));
                        self.cell(level, origin, extent, labels, cell, emit);
                    }
                }
            }
        }
    }

    fn cell(
        &mut self,
        level: Level,
        origin: [u32; 3],
        extent: [u32; 3],
        labels: &[u32],
        cell: [u32; 3],
        emit: bool,
    ) {
        let mut corners = [BACKGROUND; 8];
        let mut distinct = [BACKGROUND; 8];
        let mut count = 0usize;
        for (corner, point) in CORNER.iter().enumerate() {
            let label = voxel(extent, labels, cell, point);
            corners[corner] = label;
            if label != BACKGROUND && !distinct[..count].contains(&label) {
                distinct[count] = label;
                count += 1;
            }
        }
        for label in distinct.iter().take(count).copied() {
            let mut bits = 0usize;
            for (corner, value) in corners.iter().enumerate() {
                if *value != label {
                    bits |= 1 << corner;
                }
            }
            if EDGE_TABLE[bits] == 0 {
                continue;
            }
            let slot = self.slot(label);
            let table = TRIANGLE_TABLE[bits];
            let mut entry = 0usize;
            while entry < 16 && table[entry] >= 0 {
                let mut vertices = [0u32; 3];
                for (offset, vertex) in vertices.iter_mut().enumerate() {
                    *vertex = self.edge(
                        level,
                        origin,
                        label,
                        slot,
                        cell,
                        table[entry + offset] as usize,
                    );
                }
                entry += 3;
                if vertices[0] == vertices[1]
                    || vertices[1] == vertices[2]
                    || vertices[2] == vertices[0]
                {
                    continue;
                }
                let points = vertices.map(|vertex| self.vertices[vertex as usize]);
                let cross = (Vec3::from(points[1]) - Vec3::from(points[0]))
                    .cross(Vec3::from(points[2]) - Vec3::from(points[0]));
                for vertex in vertices {
                    let normal = &mut self.normals[vertex as usize];
                    for axis in 0..3 {
                        normal[axis] += cross[axis];
                    }
                }
                if emit {
                    self.indices.extend_from_slice(&vertices);
                    self.volume[slot] += f64::from(
                        Vec3::from(points[0])
                            .dot(Vec3::from(points[1]).cross(Vec3::from(points[2]))),
                    ) / 6.0;
                }
            }
        }
    }

    fn slot(&mut self, label: u32) -> usize {
        if let Some(slot) = self.slots.get(&label) {
            return *slot;
        }
        let slot = self.labels.len();
        self.slots.insert(label, slot);
        self.labels.push(label);
        self.volume.push(0.0);
        slot
    }

    fn edge(
        &mut self,
        level: Level,
        origin: [u32; 3],
        label: u32,
        slot: usize,
        cell: [u32; 3],
        edge: usize,
    ) -> u32 {
        let [first, second] = EDGE_CORNER[edge];
        let corner = [CORNER[first], CORNER[second]];
        let mut low = [0u32; 3];
        let mut axis = 0usize;
        for component in 0..3 {
            low[component] =
                cell[component] + corner[0][component].min(corner[1][component]) as u32;
            if corner[0][component] != corner[1][component] {
                axis = component;
            }
        }
        let absolute = [0, 1, 2].map(|component| origin[component] + low[component]);
        assert!(
            label < 1 << 30,
            "a traced label addresses a surface within thirty bits"
        );
        let key = (u128::from(label) << 98)
            | (u128::from(absolute[0]) << 66)
            | (u128::from(absolute[1]) << 34)
            | (u128::from(absolute[2]) << 2)
            | axis as u128;
        if let Some(vertex) = self.edges.get(&key) {
            return *vertex;
        }
        let point = [0, 1, 2].map(|component| {
            let offset = if component == axis { 0.5 } else { 0.0 };
            absolute[component] as f32 + offset
        });
        let vertex = self.vertices.len() as u32;
        self.vertices.push(level.voxel_center(point));
        self.normals.push([0.0; 3]);
        self.objects.push(slot as u32);
        self.edges.insert(key, vertex);
        vertex
    }

    pub fn finish(mut self) -> Option<Traced> {
        if self.indices.is_empty() {
            return None;
        }
        let flip = self
            .volume
            .iter()
            .map(|volume| *volume < 0.0)
            .collect::<Vec<_>>();
        for triangle in self.indices.as_chunks_mut::<3>().0 {
            if flip[self.objects[triangle[0] as usize] as usize] {
                triangle.swap(1, 2);
            }
        }
        let vertices = self
            .vertices
            .iter()
            .zip(&self.normals)
            .zip(&self.objects)
            .map(|((position, normal), object)| {
                let normal = Vec3::from(*normal);
                let normal = normal.normalize_or(Vec3::Z);
                MeshVertex::new(*position, normal.to_array(), *object)
            })
            .collect();
        Some(Traced {
            mesh: Mesh::new(vertices, self.indices),
            labels: self.labels,
        })
    }
}

fn voxel(extent: [u32; 3], labels: &[u32], cell: [u32; 3], corner: &[i64; 3]) -> u32 {
    let mut local = [0usize; 3];
    for axis in 0..3 {
        let value = i64::from(cell[axis]) + corner[axis];
        if value < 0 || value >= i64::from(extent[axis]) {
            return BACKGROUND;
        }
        local[axis] = value as usize;
    }
    labels[local[0] + extent[0] as usize * (local[1] + extent[1] as usize * local[2])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use grana_volume::{Extent, Spacing};

    const SIDE: u32 = 16;

    fn level() -> Level {
        Level::new(Extent::new(SIDE, SIDE, SIDE), Spacing::new(1.0, 1.0, 1.0))
    }

    fn slab() -> Vec<u32> {
        let mut labels = vec![0u32; (SIDE * SIDE * SIDE) as usize];
        for y in 4..12u32 {
            for x in 4..12u32 {
                labels[(x + SIDE * (y + SIDE * 8)) as usize] = 1;
            }
        }
        labels
    }

    fn trace(labels: &[u32], cells: [std::ops::Range<u32>; 3]) -> Traced {
        let mut tracer = Tracer::new();
        tracer.push(TraceBand::new(
            level(),
            [0, 0, 0],
            [SIDE, SIDE, SIDE],
            cells,
            labels,
        ));
        tracer.finish().expect("the labels trace a surface")
    }

    fn bounds(traced: &Traced) -> ([f32; 3], [f32; 3]) {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for vertex in traced.mesh.vertices() {
            for (axis, value) in vertex.position().into_iter().enumerate() {
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
        (low, high)
    }

    #[test]
    fn keeps_a_one_voxel_sheet_as_its_own_thin_surface() {
        let traced = trace(&slab(), [0..SIDE - 1, 0..SIDE - 1, 0..SIDE - 1]);
        assert_eq!(traced.labels, vec![1], "one label carries one surface");
        let (low, high) = bounds(&traced);
        assert!(
            high[0] - low[0] >= 7.9 && high[1] - low[1] >= 7.9,
            "the sheet spans the voxels it holds: {low:?}..{high:?}"
        );
        assert!(
            high[2] - low[2] <= 2.0,
            "a one voxel sheet traces into a thin surface, not a block: {low:?}..{high:?}"
        );
        assert!(
            traced.mesh.triangle_count() > 0,
            "the sheet carries triangles"
        );
    }

    #[test]
    fn separates_neighbouring_labels_and_orients_their_normals_outward() {
        let mut labels = slab();
        for y in 4..12u32 {
            for x in 12..16u32 {
                labels[(x + SIDE * (y + SIDE * 8)) as usize] = 2;
            }
        }
        let traced = trace(&labels, [0..SIDE - 1, 0..SIDE - 1, 0..SIDE - 1]);
        assert_eq!(traced.labels, vec![1, 2], "each label names its own object");
        let mut outward = [0.0f64; 2];
        let mut count = [0usize; 2];
        let mut centre = [[0.0f64; 3]; 2];
        for vertex in traced.mesh.vertices() {
            let object = vertex.object() as usize;
            for (axis, value) in vertex.position().into_iter().enumerate() {
                centre[object][axis] += f64::from(value);
            }
            count[object] += 1;
        }
        for (centre, count) in centre.iter_mut().zip(&count) {
            for axis in centre.iter_mut() {
                *axis /= *count as f64;
            }
        }
        for vertex in traced.mesh.vertices() {
            let object = vertex.object() as usize;
            for (axis, value) in vertex.position().into_iter().enumerate() {
                outward[object] +=
                    f64::from(vertex.normal()[axis]) * (f64::from(value) - centre[object][axis]);
            }
        }
        for (object, outward) in outward.iter().enumerate() {
            assert!(
                *outward > 0.0,
                "the normals of object {object} point away from its surface"
            );
        }
    }

    #[test]
    fn emits_a_shared_cell_once_across_bands() {
        let labels = slab();
        let mut whole = Tracer::new();
        whole.push(TraceBand::new(
            level(),
            [0, 0, 0],
            [SIDE, SIDE, SIDE],
            [0..SIDE - 1, 0..SIDE - 1, 0..SIDE - 1],
            &labels,
        ));
        let whole = whole.finish().expect("the labels trace a surface");
        let mut split = Tracer::new();
        for (low, high) in [(0u32, 8u32), (8, SIDE - 1)] {
            split.push(TraceBand::new(
                level(),
                [0, 0, 0],
                [SIDE, SIDE, SIDE],
                [0..SIDE - 1, 0..SIDE - 1, low..high],
                &labels,
            ));
        }
        let split = split.finish().expect("the labels trace a surface");
        assert_eq!(
            whole.mesh.triangle_count(),
            split.mesh.triangle_count(),
            "a band boundary does not emit a cell twice"
        );
        assert_eq!(
            whole.mesh.vertex_count(),
            split.mesh.vertex_count(),
            "a band boundary shares the vertices it names"
        );
    }
}
