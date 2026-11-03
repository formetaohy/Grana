use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::{Reference, Shape, relate_frames};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_object::Spot;

#[test]
fn measures_the_distance_and_containment_of_spots() {
    let reference = Reference::new(&Shape::mesh(Arc::new(cubes(&[(0, Vec3::ZERO, 2.0)]))));
    let spots = Arc::from([
        Spot::new([5.0, 0.0, 0.0], 0.5, 1.0),
        Spot::new([0.0, 0.0, 0.0], 0.5, 1.0),
    ]);
    let frame = reference.relate(&Shape::spots(spots));
    assert_eq!(frame.objects(), 2);
    assert!(
        (frame.distance(0) - 3.0).abs() < 1.0e-3,
        "{}",
        frame.distance(0)
    );
    assert_eq!(frame.inside(0), 0.0);
    assert_eq!(frame.partner(0), 1);
    assert_eq!(frame.distance(1), 0.0);
    assert_eq!(frame.inside(1), 1.0);
    assert_eq!(frame.partner(1), 1);
}

#[test]
fn measures_the_distance_and_containment_of_meshes() {
    let reference = Reference::new(&Shape::mesh(Arc::new(cubes(&[(0, Vec3::ZERO, 2.0)]))));
    let outside = Shape::mesh(Arc::new(cubes(&[(0, Vec3::new(8.0, 0.0, 0.0), 1.0)])));
    let frame = reference.relate(&outside);
    assert_eq!(frame.objects(), 1);
    assert!(
        (frame.distance(0) - 5.0).abs() < 1.0e-2,
        "{}",
        frame.distance(0)
    );
    assert_eq!(frame.inside(0), 0.0);
    assert_eq!(frame.partner(0), 1);
    let inside = Shape::mesh(Arc::new(cubes(&[(0, Vec3::new(0.5, 0.0, 0.0), 1.0)])));
    let frame = reference.relate(&inside);
    assert_eq!(frame.distance(0), 0.0);
    assert_eq!(frame.inside(0), 1.0);
}

#[test]
fn an_empty_reference_reaches_nothing() {
    let reference = Reference::new(&Shape::mesh(Arc::new(cubes(&[]))));
    let spots = Arc::from([Spot::new([1.0, 0.0, 0.0], 0.5, 1.0)]);
    let frame = reference.relate(&Shape::spots(spots));
    assert_eq!(frame.objects(), 1);
    assert!(frame.distance(0).is_infinite());
    assert_eq!(frame.inside(0), 0.0);
    assert_eq!(frame.partner(0), 0);
}

#[test]
fn keeps_relations_deterministic_across_worker_counts() {
    let target = (0..4)
        .map(|index| {
            Some(Shape::mesh(Arc::new(cubes(&[(
                0,
                Vec3::new(6.0 * index as f32, 0.0, 0.0),
                2.0,
            )]))))
        })
        .collect::<Vec<_>>();
    let source = (0..4)
        .map(|index| {
            Some(Shape::spots(Arc::from([Spot::new(
                [6.0 * index as f32 + 5.0, 0.0, 0.0],
                0.5,
                1.0,
            )])))
        })
        .collect::<Vec<_>>();
    let cancel = AtomicBool::new(false);
    let counts = source
        .iter()
        .map(|frame| frame.as_ref().map(Shape::objects))
        .collect::<Vec<_>>();
    let single = relate_frames(
        &counts,
        |time| Ok(source[time].clone()),
        |time| Ok(target[time].clone()),
        1,
        &cancel,
        &mut |_, _| {},
    )
    .expect("the relation frames are measured")
    .expect("the relation finishes");
    let many = relate_frames(
        &counts,
        |time| Ok(source[time].clone()),
        |time| Ok(target[time].clone()),
        8,
        &cancel,
        &mut |_, _| {},
    )
    .expect("the relation frames are measured")
    .expect("the relation finishes");
    assert_eq!(single, many, "the relation does not depend on workers");
    assert_eq!(single.len(), 4);
    assert!(
        (single[2].as_ref().expect("the frame relates").distance(0) - 3.0).abs() < 1.0e-2,
        "every frame measures its own target"
    );
}

fn cubes(cubes: &[(u32, Vec3, f32)]) -> PackedMesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, offset, half) in cubes {
        let base = vertices.len() as u32;
        for bits in 0..8usize {
            let corner = Vec3::new(
                if bits & 1 == 0 { -half } else { *half },
                if bits & 2 == 0 { -half } else { *half },
                if bits & 4 == 0 { -half } else { *half },
            ) + *offset;
            vertices.push(MeshVertex::new(corner.to_array(), [0.0, 0.0, 1.0], *object));
        }
        let quads = [
            [0, 1, 3, 2],
            [6, 7, 5, 4],
            [0, 4, 5, 1],
            [3, 7, 6, 2],
            [2, 6, 4, 0],
            [1, 5, 7, 3],
        ];
        for quad in quads {
            for corner in [0, 1, 2, 0, 2, 3] {
                indices.push(base + quad[corner]);
            }
        }
    }
    PackedMesh::new(Mesh::new(vertices, indices))
}
