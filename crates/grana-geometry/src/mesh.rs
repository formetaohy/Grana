use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    object: u32,
}

impl MeshVertex {
    pub fn new(position: [f32; 3], normal: [f32; 3], object: u32) -> Self {
        assert!(
            position
                .iter()
                .chain(normal.iter())
                .all(|value| value.is_finite()),
            "a mesh vertex is finite"
        );
        let length_squared = normal.iter().map(|value| value * value).sum::<f32>();
        assert!(
            (length_squared - 1.0).abs() < 1.0e-3,
            "a mesh normal has unit length"
        );
        Self {
            position,
            normal,
            object,
        }
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }

    pub fn normal(self) -> [f32; 3] {
        self.normal
    }

    pub fn object(self) -> u32 {
        self.object
    }
}

const _: () = assert!(size_of::<MeshVertex>() == 28);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
}

impl Mesh {
    pub fn new(vertices: Vec<MeshVertex>, indices: Vec<u32>) -> Self {
        assert!(
            indices.len().is_multiple_of(3),
            "a mesh holds whole triangles"
        );
        assert!(
            indices
                .iter()
                .all(|index| (*index as usize) < vertices.len()),
            "every mesh index addresses a vertex"
        );
        Self { vertices, indices }
    }

    pub fn vertices(&self) -> &[MeshVertex] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle(&self, index: usize) -> [MeshVertex; 3] {
        [0, 1, 2].map(|corner| self.vertices[self.indices[index * 3 + corner] as usize])
    }
}
