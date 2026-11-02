use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{IntensityTable, SampleAccumulator, SurfaceSet, SurfaceStatistics};
use grana_scene::{Dataset, Item, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Axis, Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const CORNERS: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

const FACES: [[u32; 3]; 12] = [
    [0, 1, 2],
    [0, 2, 3],
    [4, 6, 5],
    [4, 7, 6],
    [0, 4, 5],
    [0, 5, 1],
    [1, 5, 6],
    [1, 6, 2],
    [2, 6, 7],
    [2, 7, 3],
    [3, 7, 4],
    [3, 4, 0],
];

#[test]
fn picks_the_innermost_object_cut_by_the_plane() {
    let extent = Extent::new(4, 4, 2);
    let volume = Volume::new(
        extent,
        Spacing::new(0.5, 0.5, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 1.0, 1.0],
            VoxelData::U16(vec![1; extent.voxel_count() as usize]),
        )],
    );
    let mut scene = Scene::new(Dataset::synthetic(
        "fixture",
        VolumeSeries::new(vec![volume], 1.0).pyramid().clone(),
    ));
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    let statistics = SurfaceStatistics::new(
        1,
        1.0,
        1.0,
        [0.0, 0.0, 0.0],
        Bounds::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
    );
    let surfaces = SurfaceSet::new(
        0,
        cubes(),
        vec![statistics; 2],
        IntensityTable::new(1, vec![accumulator.statistics(); 2]),
    );
    let set = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(set, 0, ObjectFrame::Surfaces(surfaces));

    let pick = |x: f32, y: f32, z: f32| {
        scene
            .pick_slice(Axis::Z, Vec3::new(x, y, z))
            .map(|selection| selection.item)
    };
    assert_eq!(pick(0.0, 0.0, 0.0), Some(Item::Object(1)));
    assert_eq!(pick(0.35, 0.0, 0.0), Some(Item::Object(0)));
    assert_eq!(pick(0.9, 0.9, 0.0), None);
    assert_eq!(pick(0.0, 0.0, 0.9), None);
}

fn cubes() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, radius) in [(0u32, 0.5f32), (1, 0.2)] {
        let base = vertices.len() as u32;
        for corner in CORNERS {
            let position = Vec3::from(corner) * radius;
            vertices.push(MeshVertex::new(
                position.to_array(),
                position.normalize().to_array(),
                object,
            ));
        }
        for face in FACES {
            indices.extend(face.map(|index| base + index));
        }
    }
    Mesh::new(vertices, indices)
}
