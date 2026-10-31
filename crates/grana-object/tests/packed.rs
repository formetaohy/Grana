use grana_object::{Mesh, MeshVertex, PackedMesh};

#[test]
fn packs_objects_with_bounded_error() {
    let mesh = mesh(&[8, 8]);
    let raw = std::mem::size_of_val(mesh.vertices()) + std::mem::size_of_val(mesh.indices());
    let packed = PackedMesh::new(mesh.clone());
    assert_eq!(packed.object_count(), 2);
    assert_eq!(packed.vertex_count(), mesh.vertex_count());
    assert_eq!(packed.triangle_count(), mesh.triangle_count());
    assert!(packed.wide().is_empty());
    assert!(packed.bytes() < raw as u64);
    assert_geometry(&mesh, &packed.expand());
}

#[test]
fn widens_objects_above_the_narrow_index_range() {
    let mesh = mesh(&[70_000]);
    let packed = PackedMesh::new(mesh.clone());
    assert!(
        packed.narrow().is_empty(),
        "a wide object uses wide indices"
    );
    assert!(!packed.wide().is_empty());
    assert_geometry(&mesh, &packed.expand());
}

fn assert_geometry(source: &Mesh, expanded: &Mesh) {
    assert_eq!(expanded.vertex_count(), source.vertex_count());
    assert_eq!(expanded.triangle_count(), source.triangle_count());
    let objects = source
        .vertices()
        .iter()
        .map(|vertex| vertex.object())
        .max()
        .map_or(0, |object| object as usize + 1);
    let mut low = vec![[f32::INFINITY; 3]; objects];
    let mut high = vec![[f32::NEG_INFINITY; 3]; objects];
    for vertex in source.vertices() {
        let object = vertex.object() as usize;
        for axis in 0..3 {
            low[object][axis] = low[object][axis].min(vertex.position()[axis]);
            high[object][axis] = high[object][axis].max(vertex.position()[axis]);
        }
    }
    let tolerance =
        |object: usize, axis: usize| (high[object][axis] - low[object][axis]) / 65535.0 + 1.0e-6;
    for (original, vertex) in source.vertices().iter().zip(expanded.vertices()) {
        assert_eq!(original.object(), vertex.object());
        let dot = (0..3)
            .map(|axis| original.normal()[axis] * vertex.normal()[axis])
            .sum::<f32>();
        assert!(dot > 0.999, "the packed normal drifts to {dot}");
        for axis in 0..3 {
            let offset = (original.position()[axis] - vertex.position()[axis]).abs();
            assert!(
                offset <= tolerance(original.object() as usize, axis),
                "the packed position drifts by {offset}"
            );
        }
    }
    for index in 0..source.triangle_count() {
        let original = source.triangle(index);
        let decoded = expanded.triangle(index);
        for (a, b) in original.iter().zip(decoded.iter()) {
            assert_eq!(a.object(), b.object(), "a packed triangle changes object");
            for axis in 0..3 {
                let offset = (a.position()[axis] - b.position()[axis]).abs();
                assert!(
                    offset <= tolerance(a.object() as usize, axis),
                    "a packed triangle drifts by {offset}"
                );
            }
        }
    }
}

fn mesh(counts: &[usize]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, count) in counts.iter().copied().enumerate() {
        let base = vertices.len() as u32;
        for step in 0..count {
            vertices.push(MeshVertex::new(
                [step as f32, object as f32, 0.0],
                [0.0, 0.0, 1.0],
                object as u32,
            ));
        }
        for step in 0..count - 2 {
            let first = base + step as u32;
            indices.extend([first, first + 1, first + 2]);
        }
    }
    Mesh::new(vertices, indices)
}
