use glam::Vec3;
use grana_geometry::{Mesh, MeshVertex, PackedMesh, SectionCutter, SectionPlane};

fn cube(half: f32) -> Mesh {
    let corners = [
        [-half, -half, -half],
        [half, -half, -half],
        [half, half, -half],
        [-half, half, -half],
        [-half, -half, half],
        [half, -half, half],
        [half, half, half],
        [-half, half, half],
    ];
    let vertices = corners
        .into_iter()
        .map(|corner| MeshVertex::new(corner, [0.0, 0.0, 1.0], 0))
        .collect();
    let indices = vec![
        0, 3, 2, 0, 2, 1, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6,
        3, 0, 4, 3, 4, 7,
    ];
    Mesh::new(vertices, indices)
}

fn area(vertices: &[[f32; 2]]) -> f32 {
    let (quads, remainder) = vertices.as_chunks::<4>();
    assert!(remainder.is_empty(), "cut faces are quads");
    let mut total = 0.0f32;
    for quad in quads {
        let corners = (*quad).map(glam::Vec2::from);
        let edges = [
            corners[1] - corners[0],
            corners[2] - corners[1],
            corners[3] - corners[2],
            corners[0] - corners[3],
        ];
        total += 0.5 * edges[0].perp_dot(edges[1]) + 0.5 * edges[2].perp_dot(edges[3]);
    }
    total.abs()
}

#[test]
fn cuts_a_box_along_an_oblique_plane() {
    let mesh = PackedMesh::new(cube(1.0));
    let normal = Vec3::new(1.0, 1.0, 0.0).normalize();
    let plane = SectionPlane::new(normal, Vec3::X, 0.0);
    let mut cutter = SectionCutter::new();
    let mut vertices = Vec::new();
    let quads = cutter.cut(&mesh, 0, &plane, &mut vertices);
    assert_eq!(quads, 1, "a box cut by one plane spans one quad");
    let cut = area(&vertices);
    let expected = 4.0 * 2.0f32.sqrt();
    assert!(
        (cut - expected).abs() < 1.0e-3 * expected,
        "an oblique cut covers {cut} of the expected {expected}"
    );
}
