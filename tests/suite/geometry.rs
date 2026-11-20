use glam::Vec3;
use grana_geometry::{Hull, Mesh, MeshVertex, simplify};
use grana_object::Mask;
use grana_volume::{Extent, Level, Region, Spacing};

fn torus(object: u32, major: u32, minor: u32, offset: f32) -> Mesh {
    let vertex = |u: u32, v: u32| {
        let around = std::f32::consts::TAU * u as f32 / major as f32;
        let across = std::f32::consts::TAU * v as f32 / minor as f32;
        let reach = 5.0 + 1.5 * across.cos();
        [
            offset + reach * around.cos(),
            reach * around.sin(),
            1.5 * across.sin(),
        ]
    };
    let mut vertices = Vec::with_capacity((major * minor) as usize);
    for u in 0..major {
        for v in 0..minor {
            let at = vertex(u, v);
            let next_u = Vec3::from(vertex((u + 1) % major, v));
            let next_v = Vec3::from(vertex(u, (v + 1) % minor));
            let normal = (next_u - Vec3::from(at)).cross(next_v - Vec3::from(at));
            vertices.push(MeshVertex::new(at, normal.normalize().to_array(), object));
        }
    }
    let mut indices = Vec::new();
    for u in 0..major {
        for v in 0..minor {
            let corner = |du: u32, dv: u32| ((u + du) % major) * minor + (v + dv) % minor;
            indices.extend([corner(0, 0), corner(1, 0), corner(0, 1)]);
            indices.extend([corner(1, 0), corner(1, 1), corner(0, 1)]);
        }
    }
    Mesh::new(vertices, indices)
}

fn torus_gap(point: [f32; 3]) -> f32 {
    let point = Vec3::from(point);
    (((point.x * point.x + point.y * point.y).sqrt() - 5.0).powi(2) + point.z * point.z).sqrt()
        - 1.5
}

#[test]
fn simplifies_a_dense_torus_without_losing_its_shape() {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for part in [torus(0, 96, 48, 0.0), torus(1, 96, 48, 20.0)] {
        let base = vertices.len() as u32;
        vertices.extend_from_slice(part.vertices());
        indices.extend(part.indices().iter().map(|index| index + base));
    }
    let mesh = Mesh::new(vertices, indices);
    let before = mesh.triangle_count();
    let reduced = simplify(&mesh, before / 8);
    let after = reduced.triangle_count();
    assert!(
        after < before / 4,
        "the mesh barely simplified: {after} of {before}"
    );
    assert!(
        after > before / 32,
        "the mesh collapsed too far: {after} of {before}"
    );
    let objects = reduced
        .vertices()
        .iter()
        .map(|vertex| vertex.object())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(objects.len(), 2, "an object vanished in simplification");
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for vertex in reduced.vertices() {
        let point = vertex.position();
        assert!(point.iter().all(|value| value.is_finite()));
        let gap = torus_gap(point)
            .abs()
            .min(torus_gap([point[0] - 20.0, point[1], point[2]]).abs());
        assert!(
            gap < 0.6,
            "a simplified vertex leaves the torus by {gap} at {point:?}"
        );
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    assert!(
        (high[0] - low[0] - 33.0).abs() < 1.0,
        "the major extent changed: {}",
        high[0] - low[0]
    );
    assert!(
        reduced.triangle_count() * 3 == reduced.indices().len(),
        "the simplified mesh holds whole triangles"
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
    let shaped = Level::new(Extent::new(40, 24, 12), Spacing::new(0.2, 0.3, 0.7));
    for (axis, up, right, count, half_width, half_height) in [
        (0, Vec3::Z, Vec3::Y, 40u32, 3.6f32, 4.2f32),
        (1, Vec3::Z, Vec3::X, 24u32, 4.0f32, 4.2f32),
        (2, Vec3::Y, Vec3::X, 12u32, 4.0f32, 3.6f32),
    ] {
        let frame = shaped.slice_frame(axis);
        assert_eq!(frame.count(), count, "the slice spans axis {axis}");
        assert!(
            (Vec3::from(frame.up()) - up).length() < 1.0e-5,
            "slice {axis} raises {up:?} but holds {:?}",
            frame.up()
        );
        assert!(
            (Vec3::from(frame.right()) - right).length() < 1.0e-5,
            "slice {axis} runs {right:?} but holds {:?}",
            frame.right()
        );
        assert!(
            (frame.half_width() - half_width).abs() < 1.0e-4
                && (frame.half_height() - half_height).abs() < 1.0e-4,
            "slice {axis} covers {} by {}",
            frame.half_width(),
            frame.half_height()
        );
    }
}

#[test]
fn hulls_a_sphere_of_points() {
    let radius = 4.0f32;
    let mut points = Vec::new();
    for stack in 1..24u32 {
        let elevation = std::f32::consts::PI * stack as f32 / 24.0;
        for slice in 0..48u32 {
            let azimuth = std::f32::consts::TAU * slice as f32 / 48.0;
            points.push([
                radius * elevation.sin() * azimuth.cos(),
                radius * elevation.sin() * azimuth.sin(),
                radius * elevation.cos(),
            ]);
        }
    }
    let hull = Hull::of(&points).expect("a sphere of points carries a hull");
    let expected = 4.0 / 3.0 * std::f64::consts::PI * f64::from(radius).powi(3);
    let volume = hull.volume();
    assert!(
        volume < expected && volume > 0.9 * expected,
        "the hull holds {volume} of the sphere volume {expected}"
    );
    let diameter = hull.diameter();
    let expected = 2.0 * f64::from(radius);
    assert!(
        (diameter - expected).abs() < 0.05 * expected,
        "the hull diameter is {diameter} and not {expected}"
    );
}
