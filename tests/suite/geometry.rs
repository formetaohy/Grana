use glam::Vec3;
use grana_geometry::{Mesh, MeshVertex, PackedMesh, SectionCutter, SectionPlane};
use grana_object::Mask;
use grana_volume::{Extent, Level, Region, Spacing};

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

fn tilted(half: f32) -> Region {
    let across = Vec3::new(1.0, 0.0, 1.0).normalize();
    let down = Vec3::new(-1.0, 0.0, 1.0).normalize();
    Region::new(Vec3::ZERO, [across, Vec3::Y, down], Vec3::splat(half))
}

#[test]
fn describes_an_oriented_box() {
    let region = tilted(1.0);
    assert!(region.contains(Vec3::new(0.4, -0.7, 0.4)));
    assert!(!region.contains(Vec3::new(1.0, 0.0, 1.0)));
    assert!(!region.contains(Vec3::new(0.0, 1.2, 0.0)));
    for index in 0..8 {
        let corner = region.corner(index);
        let local = region.local(corner);
        assert!((local.abs() - region.half()).length() < 1.0e-5);
    }
    let (entry, exit) = region
        .interval(-5.0 * Vec3::X, Vec3::X)
        .expect("the ray crosses the box");
    let reach = 2.0f32.sqrt();
    assert!((entry - (5.0 - reach)).abs() < 1.0e-4, "entry {entry}");
    assert!((exit - (5.0 + reach)).abs() < 1.0e-4, "exit {exit}");
    assert!(
        region.interval(Vec3::new(0.0, 2.0, 0.0), Vec3::X).is_none(),
        "a ray above the box misses it"
    );
    assert!(
        Region::checked(
            [0.0; 3],
            [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [1.0; 3]
        )
        .is_none(),
        "parallel axes do not describe a box"
    );
}

#[test]
fn rasterises_an_oriented_region_exactly() {
    let extent = Extent::new(24, 20, 16);
    let spacing = Spacing::new(0.5, 0.5, 0.5);
    let level = Level::new(extent, spacing);
    let region = Region::new(
        Vec3::new(0.5, -0.25, 0.75),
        [
            Vec3::new(1.0, 0.0, 1.0).normalize(),
            Vec3::Y,
            Vec3::new(-1.0, 0.0, 1.0).normalize(),
        ],
        Vec3::new(3.0, 2.5, 4.0),
    );
    let mask = Mask::boxed(region, level);
    let mut inside = 0;
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let centre = level.voxel_center([x as f32, y as f32, z as f32]);
                let expected = region.contains(Vec3::from(centre));
                assert_eq!(
                    mask.contains(centre),
                    expected,
                    "the voxel at {x},{y},{z} disagrees with the box"
                );
                inside += usize::from(expected);
            }
        }
    }
    assert!(inside > 0, "the tilted box holds no voxel");
    assert!(
        inside < extent.voxel_count() as usize,
        "the tilted box holds the whole volume"
    );
}
