use std::sync::Arc;

use grana_analysis::{Roi, Shape, SpotParams, SpotScan};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_object::Spot;
use grana_volume::{Extent, Level, Region, Spacing};

const SIDE: u32 = 32;
const HIGH: f32 = 65_535.0;

fn level() -> Level {
    Level::new(Extent::new(SIDE, SIDE, SIDE), Spacing::new(1.0, 1.0, 1.0))
}

fn centre(index: u32) -> f32 {
    index as f32 - SIDE as f32 * 0.5 + 0.5
}

fn cube(half: f32) -> PackedMesh {
    let low = -half;
    let high = half;
    let corners = [
        [low, low, low],
        [high, low, low],
        [high, high, low],
        [low, high, low],
        [low, low, high],
        [high, low, high],
        [high, high, high],
        [low, high, high],
    ];
    let faces = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let vertices = corners
        .iter()
        .map(|corner| MeshVertex::new(*corner, [0.0, 0.0, 1.0], 0))
        .collect::<Vec<_>>();
    let indices = faces.into_iter().flatten().collect::<Vec<_>>();
    PackedMesh::new(Mesh::new(vertices, indices))
}

#[test]
fn fills_the_volume_of_a_closed_mesh() {
    let mesh = cube(6.0);
    let roi = Roi::new(vec![Some(Shape::Mesh(Arc::new(mesh)))]);
    let mask = roi.mask(level(), 0);
    assert!(!mask.is_empty(), "a closed mesh of this size covers cells");
    let inside = mask.count();
    let expected = 12f64.powi(3);
    let relative = (inside as f64 - expected).abs() / expected;
    assert!(
        relative < 0.2,
        "the mask holds {inside} cells against {expected}"
    );
    assert!(mask.contains([0.0, 0.0, 0.0]), "the centre lies inside");
    assert!(
        mask.contains([centre(14), centre(20), centre(16)]),
        "an inner voxel lies inside"
    );
    assert!(
        !mask.contains([centre(1), centre(1), centre(1)]),
        "a corner voxel lies outside"
    );
    assert!(
        !mask.contains([9.0, 0.0, 0.0]),
        "a point beyond the mesh lies outside"
    );
}

#[test]
fn drops_spots_outside_the_mask() {
    let level = Level::new(Extent::new(16, 16, 16), Spacing::new(1.0, 1.0, 1.0));
    let mut values = vec![0.0f32; 16 * 16 * 16];
    for z in 7..10usize {
        for y in 7..10usize {
            for x in 7..10usize {
                values[x + 16 * (y + 16 * z)] = HIGH;
            }
        }
    }
    let params = SpotParams::new(4.0, 0.2);
    let mut scan = SpotScan::new(level, HIGH, params, Region::none());
    assert_eq!(
        scan.detect(0, &values, None).len(),
        1,
        "the scan finds the bright voxel"
    );
    let far = Spot::new([-64.0, -64.0, -64.0], 2.0, 1.0);
    let outside = Roi::new(vec![Some(Shape::spots(Arc::from([far])))]);
    let mask = outside.mask(level, 0);
    assert!(
        scan.detect(0, &values, Some(&mask)).is_empty(),
        "a mask away from the voxel drops the spot"
    );
    let position = [0.5, 0.5, 0.5];
    let here = Spot::new(position, 6.0, 1.0);
    let covering = Roi::new(vec![Some(Shape::spots(Arc::from([here])))]);
    let mask = covering.mask(level, 0);
    assert_eq!(
        scan.detect(0, &values, Some(&mask)).len(),
        1,
        "a mask around the voxel keeps the spot"
    );
}

#[test]
fn an_empty_roi_covers_nothing() {
    let roi = Roi::new(vec![None]);
    let mask = roi.mask(level(), 0);
    assert!(mask.is_empty());
    assert!(!mask.contains([0.0, 0.0, 0.0]));
}
