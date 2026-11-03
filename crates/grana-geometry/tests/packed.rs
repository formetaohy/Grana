use glam::Vec3;
use grana_geometry::{Geometry, Mesh, MeshStore, MeshVertex, PackedMesh, ray_hit};

const CORNERS: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

const FACES: [[u32; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [2, 3, 7, 6],
    [1, 2, 6, 5],
    [0, 4, 7, 3],
];

#[test]
fn hits_the_surface_along_a_ray() {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for face in FACES {
        let base = vertices.len() as u32;
        for corner in face {
            vertices.push(MeshVertex::new(
                CORNERS[corner as usize],
                [0.0, 0.0, 1.0],
                0,
            ));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let packed = PackedMesh::new(Mesh::new(vertices, indices));
    let front = ray_hit(&packed, Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z)
        .expect("the ray meets the front face");
    assert!((front.z - 1.0).abs() < 1.0e-3, "the hit lies on the face");
    assert!(
        ray_hit(&packed, Vec3::new(5.0, 5.0, 5.0), Vec3::X).is_none(),
        "a ray that misses the box reports no hit"
    );
}

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
    assert_geometry(&mesh, &packed);
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
    assert_geometry(&mesh, &packed);
}

#[test]
fn pages_meshes_through_its_spill_file() {
    let store = MeshStore::temporary(1).expect("the store opens a spill file");
    let source = PackedMesh::new(mesh(&[8, 8]));
    let paged = Geometry::paged(&store, source.clone()).expect("the mesh pages");
    assert!(store.stats().spilled() > 0, "the mesh leaves memory");
    assert_eq!(store.stats().resident(), 0);
    assert_eq!(
        paged.load().expect("the mesh loads").as_ref(),
        &source,
        "a paged mesh survives the spill file"
    );
    let pruned = paged
        .without(0)
        .expect("the surface prunes")
        .expect("the object exists");
    assert_eq!(pruned.load().expect("the mesh loads").object_count(), 1);
    let recycle = MeshStore::temporary(1).expect("the store opens a spill file");
    let mut spares = (0..4)
        .map(|_| Geometry::paged(&recycle, source.clone()).expect("the mesh pages"))
        .collect::<Vec<_>>();
    let kept = spares.pop().expect("four meshes were paged");
    spares.clear();
    assert_eq!(
        kept.load().expect("the mesh loads").as_ref(),
        &source,
        "a reclaimed spill file keeps the meshes it still holds"
    );
    drop(kept);
    assert_eq!(
        recycle.stats().meshes(),
        0,
        "a released mesh leaves its store"
    );
    drop(paged);
    drop(pruned);
    assert_eq!(
        store.stats().meshes(),
        0,
        "a released mesh leaves the store"
    );
}

fn assert_geometry(source: &Mesh, packed: &PackedMesh) {
    assert_eq!(packed.vertex_count(), source.vertex_count());
    assert_eq!(packed.triangle_count(), source.triangle_count());
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
    for object in 0..objects {
        let triangles = (0..source.triangle_count())
            .map(|index| source.triangle(index))
            .filter(|triangle| triangle[0].object() as usize == object)
            .collect::<Vec<_>>();
        assert_eq!(
            triangles.len(),
            packed.groups()[object].index_count() as usize / 3,
            "every source triangle is packed into its object"
        );
        for (index, triangle) in triangles.iter().enumerate() {
            let decoded = packed.triangle(object, index);
            for (corner, position) in decoded.iter().enumerate() {
                for (axis, value) in triangle[corner].position().iter().enumerate() {
                    let offset = (value - position[axis]).abs();
                    assert!(
                        offset <= tolerance(object, axis),
                        "a packed triangle drifts by {offset}"
                    );
                }
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
