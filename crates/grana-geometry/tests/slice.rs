use grana_geometry::{Mesh, MeshVertex, PackedMesh, cross_section};

#[test]
fn fills_the_even_odd_cross_section_of_a_packed_object() {
    let mut vertices = Vec::new();
    let pack = PackedMesh::new(cube([0.0, 0.0, 0.0], 0.5, 0));
    assert!(cross_section(&pack, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 1.0).abs() < 1.0e-3,
        "the cut face area: {}",
        area(&vertices)
    );
    vertices.clear();
    assert_eq!(
        cross_section(&pack, 0, 2, 0.75, &mut vertices),
        0,
        "a plane beyond the cube meets nothing"
    );
    let hollow = PackedMesh::new(hole());
    assert!(cross_section(&hollow, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 0.75).abs() < 1.0e-3,
        "a cavity leaves the ring area: {}",
        area(&vertices)
    );
    let second = PackedMesh::new(cube([4.0, 0.0, 0.0], 0.5, 0));
    vertices.clear();
    assert!(cross_section(&second, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 1.0).abs() < 1.0e-3,
        "a translated object keeps its own frame: {}",
        area(&vertices)
    );
}

fn area(vertices: &[[f32; 2]]) -> f32 {
    vertices
        .as_chunks::<4>()
        .0
        .iter()
        .map(|quad| {
            let mut sum = 0.0;
            for corner in 0..4 {
                let from = quad[corner];
                let to = quad[(corner + 1) % 4];
                sum += from[0] * to[1] - to[0] * from[1];
            }
            sum.abs() * 0.5
        })
        .sum()
}

fn hole() -> Mesh {
    let outer = cube([0.0, 0.0, 0.0], 0.5, 0);
    let mut vertices = outer.vertices().to_vec();
    let mut indices = outer.indices().to_vec();
    let inner = cube([0.0, 0.0, 0.0], 0.25, 0);
    let base = vertices.len() as u32;
    vertices.extend(inner.vertices().iter().map(|vertex| {
        MeshVertex::new(
            [
                -vertex.position()[0],
                -vertex.position()[1],
                -vertex.position()[2],
            ],
            [
                -vertex.normal()[0],
                -vertex.normal()[1],
                -vertex.normal()[2],
            ],
            vertex.object(),
        )
    }));
    indices.extend(inner.indices().iter().map(|index| index + base));
    Mesh::new(vertices, indices)
}

fn cube(centre: [f32; 3], half: f32, object: u32) -> Mesh {
    let corners = [
        [-1.0f32, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];
    let vertices = corners
        .iter()
        .map(|corner| {
            MeshVertex::new(
                [0, 1, 2].map(|axis| centre[axis] + half * corner[axis]),
                [0.0, 0.0, 1.0],
                object,
            )
        })
        .collect::<Vec<_>>();
    let indices = [
        [0u32, 3, 2],
        [0, 2, 1],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [2, 3, 7],
        [2, 7, 6],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ]
    .concat();
    Mesh::new(vertices, indices)
}
