use std::sync::Arc;

use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_object::{LabelRaster, Shape, Spot};
use grana_volume::{Extent, Level, Spacing};

const SIDE: u32 = 8;

fn level() -> Level {
    Level::new(Extent::new(SIDE, SIDE, SIDE), Spacing::new(1.0, 1.0, 1.0))
}

fn label(labels: &[u16], x: u32, y: u32, z: u32) -> u16 {
    labels[(x + SIDE * (y + SIDE * z)) as usize]
}

#[test]
fn rasterizes_spots_into_label_chunks() {
    let shape = Shape::spots(
        vec![
            Spot::new([-2.5, -2.5, -2.5], 1.2, 1.0),
            Spot::new([2.5, 2.5, 2.5], 1.2, 1.0),
        ]
        .into(),
    );
    let raster = LabelRaster::new(&shape, level());
    let labels = raster.render(0..SIDE, 0..SIDE, 0..SIDE);
    assert_eq!(
        label(&labels, 1, 1, 1),
        1,
        "the first spot labels its voxels"
    );
    assert_eq!(
        label(&labels, 6, 6, 6),
        2,
        "the second spot labels its voxels"
    );
    assert_eq!(label(&labels, 4, 4, 4), 0, "the background stays empty");
    let chunk = raster.render(1..3, 1..3, 1..3);
    assert_eq!(chunk.len(), 8);
    assert_eq!(chunk[0], 1, "a chunk keeps the labels of its voxels");
}

fn cube() -> PackedMesh {
    let corners = [
        [-2.0, -2.0, -2.0],
        [2.0, -2.0, -2.0],
        [2.0, 2.0, -2.0],
        [-2.0, 2.0, -2.0],
        [-2.0, -2.0, 2.0],
        [2.0, -2.0, 2.0],
        [2.0, 2.0, 2.0],
        [-2.0, 2.0, 2.0],
    ];
    let faces = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [1, 2, 6, 5],
        [0, 4, 7, 3],
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for face in faces {
        let normal = [0.0, 0.0, 1.0];
        let base = vertices.len() as u32;
        for corner in face {
            vertices.push(MeshVertex::new(corners[corner], normal, 0));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    PackedMesh::new(Mesh::new(vertices, indices))
}

#[test]
fn rasterizes_meshes_into_label_chunks() {
    let shape = Shape::mesh(Arc::new(cube()));
    let raster = LabelRaster::new(&shape, level());
    let labels = raster.render(0..SIDE, 0..SIDE, 0..SIDE);
    assert_eq!(label(&labels, 4, 4, 4), 1, "the cube encloses its center");
    assert_eq!(label(&labels, 0, 0, 0), 0, "the outside stays empty");
    let interior = raster.render(3..5, 3..5, 3..5);
    assert!(
        interior.iter().all(|value| *value == 1),
        "the chunk lies inside the cube"
    );
}

#[test]
fn skips_the_objects_that_are_not_visible() {
    let shape = Shape::spots(
        vec![
            Spot::new([-2.5, -2.5, -2.5], 1.2, 1.0),
            Spot::new([2.5, 2.5, 2.5], 1.2, 1.0),
        ]
        .into(),
    );
    let raster = LabelRaster::new(&shape, level());
    let labels = raster.render_visible(Some(&[true, false]), 0..SIDE, 0..SIDE, 0..SIDE);
    assert_eq!(
        label(&labels, 1, 1, 1),
        1,
        "the visible spot keeps its label"
    );
    assert_eq!(
        label(&labels, 6, 6, 6),
        0,
        "the hidden spot leaves no label"
    );
}
