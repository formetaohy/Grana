use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_geometry::marching::{CORNER, EDGE_CORNER, EDGE_TABLE, TRIANGLE_TABLE};
use grana_geometry::{Mesh, MeshVertex};
use grana_volume::{Level, VoxelData};
use rayon::prelude::*;

const WORD: usize = 64;
const CHUNK: usize = 8;
const BACKGROUND: u32 = 0;
const BACKGROUND_TOLERANCE: f32 = 0.5;
const PIECE_PLANES: u32 = 3;

pub trait TraceVoxel: Copy + PartialEq + Send + Sync {
    fn sample(self) -> f32;

    fn cast(value: f32) -> Self;

    fn background(value: f32) -> Self {
        Self::cast((value + BACKGROUND_TOLERANCE).floor().max(f32::MIN))
    }

    fn label(self) -> u32 {
        self.sample().round().max(0.0) as u32
    }
}

macro_rules! integer_voxel {
    ($($format:ty),*) => {
        $(
            impl TraceVoxel for $format {
                fn sample(self) -> f32 {
                    self as f32
                }

                fn cast(value: f32) -> Self {
                    value.round().clamp(0.0, 1.0e15) as $format
                }
            }
        )*
    };
}

integer_voxel!(u8, u16, u32, u64);

macro_rules! signed_voxel {
    ($($format:ty),*) => {
        $(
            impl TraceVoxel for $format {
                fn sample(self) -> f32 {
                    self as f32
                }

                fn cast(value: f32) -> Self {
                    value.round().clamp(-1.0e15, 1.0e15) as $format
                }
            }
        )*
    };
}

signed_voxel!(i8, i16, i32, i64);

impl TraceVoxel for f32 {
    fn sample(self) -> f32 {
        self
    }

    fn cast(value: f32) -> Self {
        value
    }

    fn background(value: f32) -> Self {
        value
    }
}

impl TraceVoxel for f64 {
    fn sample(self) -> f32 {
        self as f32
    }

    fn cast(value: f32) -> Self {
        f64::from(value)
    }

    fn background(value: f32) -> Self {
        f64::from(value)
    }
}

pub struct TraceBand<'a> {
    level: Level,
    origin: [u32; 3],
    extent: [u32; 3],
    cells: [Range<u32>; 3],
    data: &'a VoxelData,
    background: f32,
}

impl<'a> TraceBand<'a> {
    pub fn new(
        level: Level,
        origin: [u32; 3],
        extent: [u32; 3],
        cells: [Range<u32>; 3],
        data: &'a VoxelData,
        background: f32,
    ) -> Self {
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
            data,
            background,
        }
    }
}

struct Field<'a, L: TraceVoxel> {
    level: Level,
    origin: [u32; 3],
    extent: [u32; 3],
    cells: [Range<u32>; 3],
    labels: &'a [L],
    background: L,
}

impl<'a, L: TraceVoxel> Field<'a, L> {
    fn of(
        level: Level,
        origin: [u32; 3],
        extent: [u32; 3],
        cells: [Range<u32>; 3],
        labels: &'a [L],
        background: f32,
    ) -> Self {
        assert_eq!(
            labels.len(),
            extent
                .iter()
                .map(|value| *value as usize)
                .product::<usize>(),
            "a label band holds every voxel of its extent"
        );
        Self {
            level,
            origin,
            extent,
            cells,
            labels,
            background: L::background(background),
        }
    }

    fn stride(&self) -> [usize; 3] {
        let dims = [0, 1, 2].map(|axis| self.extent[axis] as usize);
        [1, dims[0], dims[0] * dims[1]]
    }

    fn label(&self, cell: [u32; 3], corner: &[i64; 3]) -> u32 {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = i64::from(cell[axis]) + corner[axis];
            if value < 0 || value >= i64::from(self.extent[axis]) {
                return BACKGROUND;
            }
            local[axis] = value as usize;
        }
        let value = self.labels
            [local[0] + self.extent[0] as usize * (local[1] + self.extent[1] as usize * local[2])];
        match value == self.background {
            true => BACKGROUND,
            false => value.label(),
        }
    }
}

struct Active {
    low: [u32; 3],
    span: [usize; 3],
    words: usize,
    bits: Vec<u64>,
}

impl Active {
    fn of<L: TraceVoxel>(field: &Field<'_, L>, cells: &Range<u32>) -> Self {
        let low = [field.cells[0].start, field.cells[1].start, cells.start];
        let span = [
            (field.cells[0].end - low[0]) as usize,
            (field.cells[1].end - low[1]) as usize,
            (cells.end - cells.start) as usize,
        ];
        let words = span[0].div_ceil(WORD);
        let mut active = Self {
            low,
            span,
            words,
            bits: vec![0u64; words * span[1] * span[2]],
        };
        let high = [
            low[0] + span[0] as u32 - 1,
            low[1] + span[1] as u32 - 1,
            low[2] + span[2] as u32 - 1,
        ];
        let voxels = [0, 1, 2]
            .map(|axis| low[axis].saturating_sub(1)..(high[axis] + 2).min(field.extent[axis]));
        let stride = field.stride();
        for z in voxels[2].clone() {
            for y in voxels[1].clone() {
                let base = z as usize * stride[2] + y as usize * stride[1];
                let row =
                    &field.labels[base + voxels[0].start as usize..base + voxels[0].end as usize];
                let (chunks, tail) = row.as_chunks::<CHUNK>();
                for (offset, chunk) in chunks.iter().enumerate() {
                    if chunk.iter().all(|value| *value == field.background) {
                        continue;
                    }
                    let first = voxels[0].start + offset as u32 * CHUNK as u32;
                    for (index, value) in chunk.iter().enumerate() {
                        if *value != field.background {
                            active.mark(low, high, [first + index as u32, y, z]);
                        }
                    }
                }
                let first = voxels[0].end - tail.len() as u32;
                for (index, value) in tail.iter().enumerate() {
                    if *value != field.background {
                        active.mark(low, high, [first + index as u32, y, z]);
                    }
                }
            }
        }
        active
    }

    fn mark(&mut self, low: [u32; 3], high: [u32; 3], voxel: [u32; 3]) {
        let range = [0, 1, 2].map(|axis| {
            voxel[axis].saturating_sub(1).max(low[axis])..=(voxel[axis] + 1).min(high[axis])
        });
        for z in range[2].clone() {
            for y in range[1].clone() {
                let line = (z - low[2]) as usize * self.span[1] + (y - low[1]) as usize;
                for x in range[0].clone() {
                    let column = (x - low[0]) as usize;
                    self.bits[line * self.words + column / WORD] |= 1u64 << (column % WORD);
                }
            }
        }
    }
}

#[derive(Default)]
struct Piece {
    labels: Vec<u32>,
    slots: HashMap<u32, usize>,
    positions: Vec<[f32; 3]>,
    objects: Vec<u32>,
    keys: HashMap<u128, u32>,
    lower: Vec<(u32, u128)>,
    upper: Vec<(u32, u128)>,
    indices: Vec<u32>,
}

impl Piece {
    fn trace<L: TraceVoxel>(field: &Field<'_, L>, cells: Range<u32>) -> Self {
        let mut piece = Self::default();
        let active = Active::of(field, &cells);
        let planes = field.origin[2] + cells.start..field.origin[2] + cells.end;
        for z in cells {
            for y in field.cells[1].clone() {
                let row =
                    (z - active.low[2]) as usize * active.span[1] + (y - active.low[1]) as usize;
                for word in 0..active.words {
                    let mut bits = active.bits[row * active.words + word];
                    while bits != 0 {
                        let column = word * WORD + bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        let cell = [active.low[0] + column as u32, y, z];
                        piece.cell(field, cell, &planes);
                    }
                }
            }
        }
        piece
    }

    fn cell<L: TraceVoxel>(&mut self, field: &Field<'_, L>, cell: [u32; 3], planes: &Range<u32>) {
        let mut corners = [BACKGROUND; 8];
        let mut distinct = [BACKGROUND; 8];
        let mut count = 0usize;
        for (corner, point) in CORNER.iter().enumerate() {
            let label = field.label(cell, point);
            corners[corner] = label;
            if label != BACKGROUND && !distinct[..count].contains(&label) {
                distinct[count] = label;
                count += 1;
            }
        }
        for label in distinct[..count].iter().copied() {
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
                        field,
                        label,
                        slot,
                        cell,
                        table[entry + offset] as usize,
                        planes,
                    );
                }
                entry += 3;
                if vertices[0] == vertices[1]
                    || vertices[1] == vertices[2]
                    || vertices[2] == vertices[0]
                {
                    continue;
                }
                self.indices.extend_from_slice(&vertices);
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
        slot
    }

    fn edge<L: TraceVoxel>(
        &mut self,
        field: &Field<'_, L>,
        label: u32,
        slot: usize,
        cell: [u32; 3],
        edge: usize,
        planes: &Range<u32>,
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
        let absolute = [0, 1, 2].map(|component| field.origin[component] + low[component]);
        assert!(
            label < 1 << 30,
            "a traced label addresses a surface within thirty bits"
        );
        let key = (u128::from(label) << 98)
            | (u128::from(absolute[0]) << 66)
            | (u128::from(absolute[1]) << 34)
            | (u128::from(absolute[2]) << 2)
            | axis as u128;
        if let Some(vertex) = self.keys.get(&key) {
            return *vertex;
        }
        let point = [0, 1, 2].map(|component| {
            let offset = if component == axis { 0.5 } else { 0.0 };
            absolute[component] as f32 + offset
        });
        let vertex = self.positions.len() as u32;
        self.positions.push(field.level.voxel_center(point));
        self.objects.push(slot as u32);
        self.keys.insert(key, vertex);
        if absolute[2] == planes.start {
            self.lower.push((vertex, key));
        }
        if absolute[2] == planes.end {
            self.upper.push((vertex, key));
        }
        vertex
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
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    objects: Vec<u32>,
    keys: HashMap<u128, u32>,
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
        self.positions.len()
    }

    pub fn push(&mut self, band: TraceBand<'_>) {
        let TraceBand {
            level,
            origin,
            extent,
            cells,
            data,
            background,
        } = band;
        macro_rules! field {
            ($values:expr) => {
                Field::of(level, origin, extent, cells, $values, background)
            };
        }
        match data {
            VoxelData::U8(values) => self.band(field!(values)),
            VoxelData::U16(values) => self.band(field!(values)),
            VoxelData::U32(values) => self.band(field!(values)),
            VoxelData::U64(values) => self.band(field!(values)),
            VoxelData::I8(values) => self.band(field!(values)),
            VoxelData::I16(values) => self.band(field!(values)),
            VoxelData::I32(values) => self.band(field!(values)),
            VoxelData::I64(values) => self.band(field!(values)),
            VoxelData::F32(values) => self.band(field!(values)),
            VoxelData::F64(values) => self.band(field!(values)),
        }
    }

    fn band<L: TraceVoxel>(&mut self, field: Field<'_, L>) {
        let cells = field.cells[2].clone();
        let planes = cells.end - cells.start;
        let threads = rayon::current_num_threads().max(1) as u32;
        let pieces = (planes / PIECE_PLANES).clamp(1, threads);
        let step = planes.div_ceil(pieces);
        let ranges = (0..pieces)
            .map(|index| {
                let start = cells.start + index * step;
                start..(start + step).min(cells.end)
            })
            .filter(|range| !range.is_empty())
            .collect::<Vec<_>>();
        let traced = ranges
            .into_par_iter()
            .map(|range| Piece::trace(&field, range))
            .collect::<Vec<_>>();
        for piece in traced {
            self.absorb(piece);
        }
    }

    fn absorb(&mut self, piece: Piece) {
        let Piece {
            labels,
            positions,
            objects,
            lower,
            upper,
            indices,
            ..
        } = piece;
        let mut mapped = Vec::with_capacity(labels.len());
        for label in labels {
            mapped.push(self.slot(label));
        }
        let mut remap = Vec::with_capacity(positions.len());
        let mut lower = lower.into_iter().peekable();
        for (local, position) in positions.into_iter().enumerate() {
            let shared = match lower.peek() {
                Some((at, key)) if *at as usize == local => {
                    let key = *key;
                    lower.next();
                    self.keys.get(&key).copied()
                }
                _ => None,
            };
            let global = match shared {
                Some(global) => global,
                None => {
                    let global = self.positions.len() as u32;
                    self.positions.push(position);
                    self.normals.push([0.0; 3]);
                    self.objects.push(mapped[objects[local] as usize] as u32);
                    global
                }
            };
            remap.push(global);
        }
        for (local, key) in upper {
            self.keys.insert(key, remap[local as usize]);
        }
        for triangle in indices.as_chunks::<3>().0 {
            let vertices = triangle.map(|local| remap[local as usize]);
            let points = vertices.map(|vertex| self.positions[vertex as usize]);
            let cross = (Vec3::from(points[1]) - Vec3::from(points[0]))
                .cross(Vec3::from(points[2]) - Vec3::from(points[0]));
            for vertex in vertices {
                let normal = &mut self.normals[vertex as usize];
                for axis in 0..3 {
                    normal[axis] += cross[axis];
                }
            }
            let slot = self.objects[vertices[0] as usize] as usize;
            self.volume[slot] += f64::from(
                Vec3::from(points[0]).dot(Vec3::from(points[1]).cross(Vec3::from(points[2]))),
            ) / 6.0;
            self.indices.extend_from_slice(&vertices);
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
            .positions
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

#[cfg(test)]
mod tests {
    use super::*;
    use grana_volume::{Extent, Spacing, VoxelData};

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

    fn cube() -> Vec<u32> {
        let mut labels = vec![0u32; (SIDE * SIDE * SIDE) as usize];
        for z in 4..12u32 {
            for y in 4..12u32 {
                for x in 4..12u32 {
                    labels[(x + SIDE * (y + SIDE * z)) as usize] = 1;
                }
            }
        }
        labels
    }

    fn sliced(labels: &[u32], low: u32, high: u32) -> VoxelData {
        let mut data = Vec::with_capacity(((high - low) * SIDE * SIDE) as usize);
        for z in low..high {
            for y in 0..SIDE {
                let row = (z * SIDE + y) * SIDE;
                data.extend_from_slice(&labels[row as usize..(row + SIDE) as usize]);
            }
        }
        VoxelData::U32(data)
    }

    fn traced_in_bands(labels: &[u32], band: u32) -> Traced {
        let mut tracer = Tracer::new();
        let mut z = 0u32;
        while z < SIDE {
            let top = (z + band).min(SIDE);
            let low = z.saturating_sub(1);
            let high = (top + 1).min(SIDE);
            let span = [SIDE, SIDE, high - low];
            let data = sliced(labels, low, high);
            let first = z - low;
            let last = (top - low).min(span[2] - 1);
            tracer.push(TraceBand::new(
                level(),
                [0, 0, low],
                span,
                [0..SIDE - 1, 0..SIDE - 1, first..last],
                &data,
                0.0,
            ));
            z = top;
        }
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
        let traced = traced_in_bands(&slab(), SIDE);
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
        let traced = traced_in_bands(&labels, SIDE);
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

    fn surface(traced: &Traced) -> Vec<([u32; 3], [u32; 3], u32)> {
        let mut rows = traced
            .mesh
            .vertices()
            .iter()
            .map(|vertex| {
                (
                    vertex.position().map(f32::to_bits),
                    vertex.normal().map(f32::to_bits),
                    vertex.object(),
                )
            })
            .collect::<Vec<_>>();
        rows.sort_unstable();
        rows
    }

    #[test]
    fn a_split_band_traces_the_same_surface_as_one_band() {
        let labels = cube();
        let whole = traced_in_bands(&labels, SIDE);
        let split = traced_in_bands(&labels, 8);
        assert_eq!(
            whole.mesh.triangle_count(),
            split.mesh.triangle_count(),
            "a band boundary does not emit a cell twice"
        );
        let whole = surface(&whole);
        let split = surface(&split);
        assert_eq!(
            whole.len(),
            split.len(),
            "a band boundary shares every vertex it names"
        );
        if let Some(at) = whole
            .iter()
            .zip(&split)
            .position(|(left, right)| left != right)
        {
            panic!(
                "the band rim at vertex {at} differs: {:?} against {:?}",
                whole[at], split[at]
            );
        }
    }
}
