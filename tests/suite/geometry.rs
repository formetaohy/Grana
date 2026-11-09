use glam::Vec3;
use grana_geometry::{
    Bounds, Hull, Mesh, MeshVertex, PackedMesh, SectionCutter, SectionPlane, SurfaceShape, simplify,
};
use grana_object::{IntensityTable, Mask, SampleStatistics, SurfaceSet, SurfaceStatistics};
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
fn builds_coarse_surface_details_in_the_background() {
    let mesh = torus(0, 192, 96, 0.0);
    let triangles = mesh.triangle_count();
    let statistics = SurfaceStatistics::new(
        1,
        1.0,
        1.0,
        [0.0; 3],
        Bounds::new([-6.5, -6.5, -1.5], [6.5, 6.5, 1.5]),
        None,
    );
    let intensities = IntensityTable::new(1, vec![SampleStatistics::new(1.0, 1.0, 1.0, 0.0, 1.0)]);
    let set = SurfaceSet::new(0, mesh, vec![statistics], intensities);
    let level = set.select_detail(triangles / 8);
    assert!(level >= 2, "a distant surface asks for a coarse detail");
    let (base, _) = set.detail(level).expect("the base surface loads");
    assert_eq!(base.object_count(), 1, "the detail keeps its object");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (mesh, available) = set.detail(level).expect("a surface detail loads");
        if mesh.triangle_count() <= triangles / 4 {
            assert_eq!(mesh.object_count(), 1, "the coarse detail keeps its object");
            assert!(available >= 2, "the ladder reports the built level");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the detail ladder never built"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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

#[test]
fn measures_the_shape_of_a_box() {
    let mesh = cube(1.0);
    let positions = mesh
        .vertices()
        .iter()
        .map(|vertex| vertex.position())
        .collect::<Vec<_>>();
    let mut hulled = positions.clone();
    hulled.push([0.0, 0.0, 0.0]);
    hulled.push([0.2, -0.3, 0.4]);
    let shape = SurfaceShape::of(&hulled, mesh.indices().as_chunks::<3>().0)
        .expect("a box carries a shape");
    let axis = (5.0f64 / 3.0).sqrt();
    for measured in shape.axes() {
        assert!(
            (measured - axis).abs() < 1.0e-4,
            "a box semi axis is {measured} and not {axis}"
        );
    }
    let diameter = shape.diameter();
    let expected = 12.0f64.sqrt();
    assert!(
        (diameter - expected).abs() < 1.0e-4,
        "a box diagonal is {diameter} and not {expected}"
    );
    let volume = shape.hull_volume();
    assert!(
        (volume - 8.0).abs() < 1.0e-6,
        "a unit box hull holds {volume} of the expected 8"
    );
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
