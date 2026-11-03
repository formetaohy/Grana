use grana_geometry::{Mesh, MeshVertex, PackedMesh, SectionCutter, SectionIndex};

const UNIT: f32 = 65535.0;

#[test]
fn fills_the_even_odd_cross_section_of_a_packed_object() {
    let mut cutter = SectionCutter::new();
    let mut vertices = Vec::new();
    let pack = PackedMesh::new(cube([0.0, 0.0, 0.0], 0.5, 0));
    assert!(cutter.cut(&pack, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 1.0).abs() < 1.0e-3,
        "the cut face area: {}",
        area(&vertices)
    );
    vertices.clear();
    assert_eq!(
        cutter.cut(&pack, 0, 2, 0.75, &mut vertices),
        0,
        "a plane beyond the cube meets nothing"
    );
    let hollow = PackedMesh::new(hole());
    assert!(cutter.cut(&hollow, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 0.75).abs() < 1.0e-3,
        "a cavity leaves the ring area: {}",
        area(&vertices)
    );
    let second = PackedMesh::new(cube([4.0, 0.0, 0.0], 0.5, 0));
    vertices.clear();
    assert!(cutter.cut(&second, 0, 2, 0.0, &mut vertices) > 0);
    assert!(
        (area(&vertices) - 1.0).abs() < 1.0e-3,
        "a translated object keeps its own frame: {}",
        area(&vertices)
    );
}

#[test]
fn indexes_the_same_cross_section_as_the_full_scan() {
    let mut cutter = SectionCutter::new();
    for mesh in [PackedMesh::new(hole()), PackedMesh::new(sphere())] {
        for axis in 0..3 {
            let index = SectionIndex::new(&mesh, 0, axis);
            let group = &mesh.groups()[0];
            let low = group.min()[axis];
            let high = low + UNIT * group.scale()[axis];
            for step in 0..=32 {
                let plane = low + (high - low) * step as f32 / 32.0;
                let mut direct = Vec::new();
                let mut indexed = Vec::new();
                let scanned = cutter.cut(&mesh, 0, axis, plane, &mut direct);
                let selected = cutter.cut_indexed(&mesh, 0, axis, plane, &index, &mut indexed);
                assert_eq!(scanned, selected, "the index loses a quad at {plane}");
                assert_eq!(direct, indexed, "the index cuts a different face");
            }
        }
    }
}

fn sphere() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let rings = 24u32;
    let slices = 32u32;
    for ring in 0..=rings {
        let polar = std::f32::consts::PI * ring as f32 / rings as f32;
        for slice in 0..slices {
            let azimuth = std::f32::consts::TAU * slice as f32 / slices as f32;
            let normal = [
                polar.sin() * azimuth.cos(),
                polar.sin() * azimuth.sin(),
                polar.cos(),
            ];
            vertices.push(MeshVertex::new(normal, normal, 0));
        }
    }
    for ring in 0..rings {
        for slice in 0..slices {
            let next = (slice + 1) % slices;
            let base = ring * slices;
            let following = (ring + 1) * slices;
            indices.extend_from_slice(&[
                base + slice,
                following + slice,
                following + next,
                base + slice,
                following + next,
                base + next,
            ]);
        }
    }
    Mesh::new(vertices, indices)
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
