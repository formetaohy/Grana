use glam::Vec3;
use grana_geometry::{Bvh, Primitive};

#[test]
fn measures_distance_and_containment() {
    let bvh = Bvh::new(
        cube(2.0)
            .into_iter()
            .map(|points| (7, Primitive::triangle(points))),
    );
    assert_eq!(bvh.primitives(), 12);
    let outside = Vec3::new(3.0, 0.0, 0.0);
    let (distance, object) = bvh.nearest(outside).expect("the cube is near");
    assert!((distance - 1.0).abs() < 1.0e-4, "{distance}");
    assert_eq!(object, 7);
    assert_eq!(bvh.contains(Vec3::ZERO), Some(7));
    assert_eq!(bvh.contains(outside), None);
    let sphere = Bvh::new([(2, Primitive::sphere(Vec3::ZERO, 1.0))]);
    let (distance, object) = sphere
        .nearest(Vec3::new(4.0, 0.0, 0.0))
        .expect("the sphere is near");
    assert!((distance - 3.0).abs() < 1.0e-6, "{distance}");
    assert_eq!(object, 2);
    assert_eq!(sphere.contains(Vec3::new(0.5, 0.0, 0.0)), Some(2));
    assert_eq!(sphere.contains(Vec3::new(1.5, 0.0, 0.0)), None);
    assert_eq!(Bvh::new([]).nearest(Vec3::ZERO), None);
}

#[test]
fn finds_the_nearest_of_many_objects() {
    let primitives = (0..64)
        .flat_map(|index| {
            let centre = Vec3::new(index as f32 * 8.0, 0.0, 0.0);
            cube(1.0).into_iter().map(move |triangle| {
                (
                    index,
                    Primitive::triangle(triangle.map(|point| point + centre)),
                )
            })
        })
        .collect::<Vec<_>>();
    let bvh = Bvh::new(primitives);
    let (distance, object) = bvh
        .nearest(Vec3::new(5.0 * 8.0, 4.0, 0.0))
        .expect("a cube is near");
    assert!((distance - 3.0).abs() < 1.0e-3, "{distance}");
    assert_eq!(object, 5);
    assert_eq!(bvh.contains(Vec3::new(9.0 * 8.0, 0.0, 0.0)), Some(9));
    assert_eq!(bvh.contains(Vec3::new(9.0 * 8.0, 4.0, 0.0)), None);
}

fn cube(half: f32) -> Vec<[Vec3; 3]> {
    let corner = |bits: usize| {
        Vec3::new(
            if bits & 1 == 0 { -half } else { half },
            if bits & 2 == 0 { -half } else { half },
            if bits & 4 == 0 { -half } else { half },
        )
    };
    let quads = [
        [0, 1, 3, 2],
        [6, 7, 5, 4],
        [0, 4, 5, 1],
        [3, 7, 6, 2],
        [2, 6, 4, 0],
        [1, 5, 7, 3],
    ];
    quads
        .into_iter()
        .flat_map(|quad| {
            let [first, second, third, fourth] = quad.map(corner);
            [[first, second, third], [first, third, fourth]]
        })
        .collect()
}
