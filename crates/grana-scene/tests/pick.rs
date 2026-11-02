use glam::Vec3;
use grana_display::Ray;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{
    IntensityTable, SampleAccumulator, Spot, SpotSet, SurfaceSet, SurfaceStatistics,
};
use grana_scene::{Dataset, Item, ObjectFrame, ObjectKind, Scene, Selection};
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
    let mut scene = fixture();
    let surfaces = SurfaceSet::new(
        0,
        cubes(&[(0u32, 0.5f32), (1, 0.2)]),
        vec![statistics(); 2],
        IntensityTable::new(1, vec![intensities(); 2]),
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

#[test]
fn refreshes_the_pick_index_when_the_geometry_changes() {
    let mut scene = fixture();
    let surfaces = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(surfaces, 0, ObjectFrame::Surfaces(stacked(0.0, 0.8)));

    let ray = Ray {
        origin: Vec3::new(0.0, 0.0, -8.0),
        direction: Vec3::Z,
    };
    let solid = |item: usize| Selection {
        set: surfaces,
        item: Item::Object(item),
    };
    assert_eq!(scene.pick_ray(&ray), Some(solid(0)));
    assert_eq!(
        scene.pick_slice(Axis::Z, Vec3::new(0.0, 0.0, 0.0)),
        Some(solid(0))
    );

    scene.install(surfaces, 0, ObjectFrame::Surfaces(stacked(0.8, 0.0)));
    assert_eq!(scene.pick_ray(&ray), Some(solid(1)));
    assert_eq!(
        scene.pick_slice(Axis::Z, Vec3::new(0.0, 0.0, 0.0)),
        Some(solid(1))
    );

    let spots = scene.add_object(ObjectKind::Spots, 0);
    let spot = |x: f32, y: f32| {
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![Spot::new([x, y, 0.0], 0.2, 0.5)],
            IntensityTable::new(1, vec![intensities()]),
        ))
    };
    scene.install(spots, 0, spot(1.0, 1.0));
    let spot_ray = Ray {
        origin: Vec3::new(1.0, 1.0, -8.0),
        direction: Vec3::Z,
    };
    assert_eq!(
        scene.pick_ray(&spot_ray),
        Some(Selection {
            set: spots,
            item: Item::Object(0),
        })
    );
    assert_eq!(
        scene.pick_slice(Axis::Z, Vec3::new(1.0, 1.0, 0.0)),
        Some(Selection {
            set: spots,
            item: Item::Object(0),
        })
    );
    scene.install(spots, 0, spot(3.0, 3.0));
    assert_eq!(scene.pick_ray(&spot_ray), None);
    assert_eq!(scene.pick_slice(Axis::Z, Vec3::new(1.0, 1.0, 0.0)), None);
}

fn fixture() -> Scene {
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
    Scene::new(Dataset::synthetic(
        "fixture",
        VolumeSeries::new(vec![volume], 1.0).pyramid().clone(),
    ))
}

fn statistics() -> SurfaceStatistics {
    SurfaceStatistics::new(
        1,
        1.0,
        1.0,
        [0.0, 0.0, 0.0],
        Bounds::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
    )
}

fn intensities() -> grana_object::SampleStatistics {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    accumulator.statistics()
}

fn cubes(objects: &[(u32, f32)]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, radius) in objects.iter().copied() {
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

fn stacked(first: f32, second: f32) -> SurfaceSet {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, z) in [first, second].into_iter().enumerate() {
        let base = vertices.len() as u32;
        for corner in CORNERS {
            let position = Vec3::from(corner) * 0.25 + Vec3::new(0.0, 0.0, z);
            vertices.push(MeshVertex::new(
                position.to_array(),
                position.normalize().to_array(),
                object as u32,
            ));
        }
        for face in FACES {
            indices.extend(face.map(|index| base + index));
        }
    }
    SurfaceSet::new(
        0,
        Mesh::new(vertices, indices),
        vec![statistics(); 2],
        IntensityTable::new(1, vec![intensities(); 2]),
    )
}
