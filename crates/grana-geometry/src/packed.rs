use std::any::Any;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use grana_codec::{Decoder, Encoder, FAULT_TAG, FAULT_VALUE, Fault};

use crate::Mesh;
use crate::store::{Decode, Payload};

const UNIT: f32 = 65535.0;
const NARROW_LIMIT: usize = u16::MAX as usize;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct PackedVertex {
    position: [u16; 3],
    normal: [u16; 2],
}

impl PackedVertex {
    pub fn new(position: [u16; 3], normal: [u16; 2]) -> Self {
        Self { position, normal }
    }

    pub fn position(self) -> [u16; 3] {
        self.position
    }

    pub fn normal(self) -> [u16; 2] {
        self.normal
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryGroup {
    min: [f32; 3],
    scale: [f32; 3],
    first_vertex: u32,
    vertex_count: u32,
    first_index: u32,
    index_count: u32,
    wide: bool,
}

impl GeometryGroup {
    pub fn new(
        min: [f32; 3],
        scale: [f32; 3],
        first_vertex: u32,
        vertex_count: u32,
        first_index: u32,
        index_count: u32,
        wide: bool,
    ) -> Self {
        Self {
            min,
            scale,
            first_vertex,
            vertex_count,
            first_index,
            index_count,
            wide,
        }
    }

    pub fn min(self) -> [f32; 3] {
        self.min
    }

    pub fn scale(self) -> [f32; 3] {
        self.scale
    }

    pub fn first_vertex(self) -> u32 {
        self.first_vertex
    }

    pub fn vertex_count(self) -> u32 {
        self.vertex_count
    }

    pub fn first_index(self) -> u32 {
        self.first_index
    }

    pub fn index_count(self) -> u32 {
        self.index_count
    }

    pub fn wide(self) -> bool {
        self.wide
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedMesh {
    groups: Vec<GeometryGroup>,
    vertices: Vec<PackedVertex>,
    narrow: Vec<u16>,
    wide: Vec<u32>,
}

impl PackedMesh {
    pub fn new(mesh: Mesh) -> Self {
        let vertices = mesh.vertices();
        let indices = mesh.indices();
        if indices.is_empty() {
            assert!(vertices.is_empty(), "an empty mesh carries no vertices");
            return Self::default();
        }
        let objects = vertices
            .iter()
            .map(|vertex| vertex.object() as usize)
            .max()
            .map_or(0, |object| object + 1);
        assert!(objects > 0, "a non empty mesh names an object");
        let mut counts = vec![0u32; objects];
        let mut low = vec![[f32::INFINITY; 3]; objects];
        let mut high = vec![[f32::NEG_INFINITY; 3]; objects];
        for vertex in vertices {
            let object = vertex.object() as usize;
            assert!(object < objects, "a mesh vertex names an object");
            counts[object] += 1;
            for (axis, value) in vertex.position().into_iter().enumerate() {
                low[object][axis] = low[object][axis].min(value);
                high[object][axis] = high[object][axis].max(value);
            }
        }
        assert!(
            counts.iter().all(|count| *count > 0),
            "every mesh object carries vertices"
        );
        let mut first = vec![0u32; objects];
        let mut next = 0u32;
        for (slot, count) in first.iter_mut().zip(&counts) {
            assert!((next as u64) + u64::from(*count) <= u64::from(u32::MAX));
            *slot = next;
            next += *count;
        }
        let mut scale = vec![[0.0f32; 3]; objects];
        for object in 0..objects {
            for axis in 0..3 {
                let extent = high[object][axis] - low[object][axis];
                scale[object][axis] = if extent > 0.0 { extent / UNIT } else { 0.0 };
            }
        }
        let mut local = vec![0u32; vertices.len()];
        let mut packed = vec![PackedVertex::new([0; 3], [0; 2]); vertices.len()];
        let mut cursor = first.clone();
        for (index, vertex) in vertices.iter().enumerate() {
            let object = vertex.object() as usize;
            let at = cursor[object] as usize;
            cursor[object] += 1;
            local[index] = at as u32;
            let position = [0, 1, 2].map(|axis| {
                quantize(
                    vertex.position()[axis],
                    low[object][axis],
                    scale[object][axis],
                )
            });
            packed[at] = PackedVertex::new(position, encode_normal(vertex.normal()));
        }
        let mut triangles = vec![0u32; objects];
        for triangle in indices.as_chunks::<3>().0 {
            let object = vertices[triangle[0] as usize].object() as usize;
            assert!(
                triangle
                    .iter()
                    .all(|index| vertices[*index as usize].object() as usize == object),
                "a mesh triangle belongs to one object"
            );
            triangles[object] += 1;
        }
        assert!(
            triangles.iter().all(|count| *count > 0),
            "every mesh object carries triangles"
        );
        let mut first_index = vec![0u32; objects];
        let mut next_narrow = 0u32;
        let mut next_wide = 0u32;
        for object in 0..objects {
            if counts[object] > NARROW_LIMIT as u32 {
                first_index[object] = next_wide;
                next_wide += triangles[object] * 3;
            } else {
                first_index[object] = next_narrow;
                next_narrow += triangles[object] * 3;
            }
        }
        let mut narrow = vec![0u16; next_narrow as usize];
        let mut wide = vec![0u32; next_wide as usize];
        let mut narrow_cursor = first_index.clone();
        let mut wide_cursor = first_index.clone();
        for triangle in indices.as_chunks::<3>().0 {
            let object = vertices[triangle[0] as usize].object() as usize;
            if counts[object] > NARROW_LIMIT as u32 {
                let start = wide_cursor[object] as usize;
                wide_cursor[object] += 3;
                for (offset, index) in triangle.iter().enumerate() {
                    wide[start + offset] = local[*index as usize] - first[object];
                }
            } else {
                let start = narrow_cursor[object] as usize;
                narrow_cursor[object] += 3;
                for (offset, index) in triangle.iter().enumerate() {
                    narrow[start + offset] = (local[*index as usize] - first[object]) as u16;
                }
            }
        }
        let groups = (0..objects)
            .map(|object| {
                GeometryGroup::new(
                    low[object],
                    scale[object],
                    first[object],
                    counts[object],
                    first_index[object],
                    triangles[object] * 3,
                    counts[object] > NARROW_LIMIT as u32,
                )
            })
            .collect();
        Self {
            groups,
            vertices: packed,
            narrow,
            wide,
        }
    }

    pub fn from_parts(
        groups: Vec<GeometryGroup>,
        vertices: Vec<PackedVertex>,
        narrow: Vec<u16>,
        wide: Vec<u32>,
    ) -> Option<Self> {
        let mut next_vertex = 0usize;
        let mut next_narrow = 0usize;
        let mut next_wide = 0usize;
        for group in &groups {
            if group.vertex_count == 0
                || group.index_count == 0
                || !group.index_count.is_multiple_of(3)
            {
                return None;
            }
            if group.first_vertex as usize != next_vertex {
                return None;
            }
            next_vertex = next_vertex.checked_add(group.vertex_count as usize)?;
            if next_vertex > vertices.len() {
                return None;
            }
            if group
                .min
                .iter()
                .chain(&group.scale)
                .any(|value| !value.is_finite())
                || group.scale.iter().any(|value| *value < 0.0)
                || (0..3).any(|axis| !(group.min[axis] + UNIT * group.scale[axis]).is_finite())
            {
                return None;
            }
            if group.wide {
                if group.first_index as usize != next_wide {
                    return None;
                }
                let end = next_wide.checked_add(group.index_count as usize)?;
                if end > wide.len()
                    || wide[next_wide..end]
                        .iter()
                        .any(|index| *index >= group.vertex_count)
                {
                    return None;
                }
                next_wide = end;
            } else {
                if group.vertex_count > u32::from(u16::MAX) {
                    return None;
                }
                if group.first_index as usize != next_narrow {
                    return None;
                }
                let end = next_narrow.checked_add(group.index_count as usize)?;
                if end > narrow.len()
                    || narrow[next_narrow..end]
                        .iter()
                        .any(|index| u32::from(*index) >= group.vertex_count)
                {
                    return None;
                }
                next_narrow = end;
            }
        }
        if next_vertex != vertices.len() || next_narrow != narrow.len() || next_wide != wide.len() {
            return None;
        }
        Some(Self {
            groups,
            vertices,
            narrow,
            wide,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.byte_length());
        let mut writer = Encoder::memory(&mut bytes);
        writer.count(self.groups.len());
        for group in &self.groups {
            for value in group.min.iter().chain(&group.scale) {
                writer.f32(*value);
            }
            writer.u32(group.first_vertex);
            writer.u32(group.vertex_count);
            writer.u32(group.first_index);
            writer.u32(group.index_count);
            writer.u8(u8::from(group.wide));
        }
        writer.count(self.vertices.len());
        for vertex in &self.vertices {
            for value in vertex.position.iter().chain(&vertex.normal) {
                writer.u16(*value);
            }
        }
        writer.count(self.narrow.len());
        for index in &self.narrow {
            writer.u16(*index);
        }
        writer.count(self.wide.len());
        for index in &self.wide {
            writer.u32(*index);
        }
        drop(writer);
        assert_eq!(
            bytes.len(),
            self.byte_length(),
            "a packed mesh encodes its own size"
        );
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::decode(bytes).ok()
    }

    fn decode(bytes: &[u8]) -> Result<Self, Fault> {
        let mut reader = Decoder::memory(bytes);
        let groups = reader.count(6 * size_of::<f32>() + 4 * size_of::<u32>() + 1)?;
        let mut decoded = Vec::with_capacity(groups);
        for _ in 0..groups {
            let mut values = [0.0f32; 6];
            for value in &mut values {
                *value = reader.f32()?;
            }
            let first_vertex = reader.u32()?;
            let vertex_count = reader.u32()?;
            let first_index = reader.u32()?;
            let index_count = reader.u32()?;
            let wide = match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(FAULT_TAG),
            };
            decoded.push(GeometryGroup::new(
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
                first_vertex,
                vertex_count,
                first_index,
                index_count,
                wide,
            ));
        }
        let vertices = reader.count(5 * size_of::<u16>())?;
        let mut packed = Vec::with_capacity(vertices);
        for _ in 0..vertices {
            packed.push(PackedVertex::new(
                [reader.u16()?, reader.u16()?, reader.u16()?],
                [reader.u16()?, reader.u16()?],
            ));
        }
        let narrow = reader.count(size_of::<u16>())?;
        let mut narrow_indices = Vec::with_capacity(narrow);
        for _ in 0..narrow {
            narrow_indices.push(reader.u16()?);
        }
        let wide = reader.count(size_of::<u32>())?;
        let mut wide_indices = Vec::with_capacity(wide);
        for _ in 0..wide {
            wide_indices.push(reader.u32()?);
        }
        reader.done()?;
        Self::from_parts(decoded, packed, narrow_indices, wide_indices).ok_or(FAULT_VALUE)
    }

    pub fn byte_length(&self) -> usize {
        4 * size_of::<u32>()
            + self.groups.len() * (6 * size_of::<f32>() + 4 * size_of::<u32>() + size_of::<u8>())
            + self.vertices.len() * 5 * size_of::<u16>()
            + self.narrow.len() * size_of::<u16>()
            + self.wide.len() * size_of::<u32>()
    }

    pub fn groups(&self) -> &[GeometryGroup] {
        &self.groups
    }

    pub fn vertices(&self) -> &[PackedVertex] {
        &self.vertices
    }

    pub fn narrow(&self) -> &[u16] {
        &self.narrow
    }

    pub fn wide(&self) -> &[u32] {
        &self.wide
    }

    pub fn object_count(&self) -> usize {
        self.groups.len()
    }

    pub fn without(&self, object: usize) -> Self {
        assert!(
            object < self.groups.len(),
            "the removed object names a packed object"
        );
        let mut groups = Vec::with_capacity(self.groups.len() - 1);
        let mut vertices = Vec::with_capacity(self.vertices.len());
        let mut narrow = Vec::with_capacity(self.narrow.len());
        let mut wide = Vec::with_capacity(self.wide.len());
        for (index, group) in self.groups.iter().enumerate() {
            if index == object {
                continue;
            }
            let first_vertex = vertices.len() as u32;
            let start = group.first_vertex as usize;
            vertices.extend_from_slice(&self.vertices[start..start + group.vertex_count as usize]);
            let first_index = if group.wide {
                let first = wide.len() as u32;
                let start = group.first_index as usize;
                wide.extend_from_slice(&self.wide[start..start + group.index_count as usize]);
                first
            } else {
                let first = narrow.len() as u32;
                let start = group.first_index as usize;
                narrow.extend_from_slice(&self.narrow[start..start + group.index_count as usize]);
                first
            };
            groups.push(GeometryGroup::new(
                group.min,
                group.scale,
                first_vertex,
                group.vertex_count,
                first_index,
                group.index_count,
                group.wide,
            ));
        }
        Self {
            groups,
            vertices,
            narrow,
            wide,
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.groups
            .iter()
            .map(|group| group.index_count as usize / 3)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn bytes(&self) -> u64 {
        (self.groups.len() * size_of::<GeometryGroup>()
            + self.vertices.len() * size_of::<PackedVertex>()
            + self.narrow.len() * size_of::<u16>()
            + self.wide.len() * size_of::<u32>()) as u64
    }

    pub fn triangle(&self, object: usize, triangle: usize) -> [[f32; 3]; 3] {
        let group = self
            .groups
            .get(object)
            .expect("the triangle names a packed object");
        assert!(
            triangle < group.index_count as usize / 3,
            "the triangle lies inside its packed object"
        );
        let start = group.first_index as usize + triangle * 3;
        [0, 1, 2].map(|corner| {
            let index = if group.wide {
                self.wide[start + corner]
            } else {
                u32::from(self.narrow[start + corner])
            };
            let vertex = self.vertices[group.first_vertex as usize + index as usize];
            [0, 1, 2]
                .map(|axis| group.min[axis] + f32::from(vertex.position[axis]) * group.scale[axis])
        })
    }

    pub fn position(&self, object: usize, vertex: usize) -> [f32; 3] {
        let group = self
            .groups
            .get(object)
            .expect("the vertex names a packed object");
        assert!(
            vertex < group.vertex_count as usize,
            "the vertex lies inside its packed object"
        );
        let packed = self.vertices[group.first_vertex as usize + vertex];
        [0, 1, 2].map(|axis| group.min[axis] + f32::from(packed.position[axis]) * group.scale[axis])
    }
}

fn quantize(value: f32, min: f32, scale: f32) -> u16 {
    if scale <= 0.0 {
        return 0;
    }
    ((value - min) / scale).round().clamp(0.0, UNIT) as u16
}

fn encode_normal(normal: [f32; 3]) -> [u16; 2] {
    let normal = Vec3::from(normal).normalize_or_zero();
    let normal = if normal == Vec3::ZERO {
        Vec3::Z
    } else {
        normal
    };
    let sum = normal.x.abs() + normal.y.abs() + normal.z.abs();
    let mut x = normal.x / sum;
    let mut y = normal.y / sum;
    if normal.z < 0.0 {
        let across = (1.0 - y.abs()) * sign(x);
        let down = (1.0 - x.abs()) * sign(y);
        x = across;
        y = down;
    }
    [quantize_unit(x), quantize_unit(y)]
}

fn quantize_unit(value: f32) -> u16 {
    (((value * 0.5 + 0.5).clamp(0.0, 1.0)) * UNIT).round() as u16
}

fn sign(value: f32) -> f32 {
    if value < 0.0 { -1.0 } else { 1.0 }
}

impl Payload for PackedMesh {
    fn encode(&self) -> Vec<u8> {
        self.to_bytes()
    }

    fn stored_bytes(&self) -> u64 {
        PackedMesh::byte_length(self) as u64
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl Decode for PackedMesh {
    fn decode(bytes: &[u8]) -> Option<Self> {
        Self::from_bytes(bytes)
    }
}
