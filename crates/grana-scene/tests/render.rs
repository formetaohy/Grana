use glam::Vec3;
use grana_object::{
    Bounds, IntensityTable, Mesh, MeshVertex, SampleStatistics, SurfaceSet, SurfaceStatistics,
};
use grana_render::Ray;
use grana_scene::{Dataset, Item, ObjectFrame, ObjectKind, Scene, Selection};
use grana_volume::{ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing, VoxelFormat};

#[test]
fn renders_and_picks_packed_geometry() {
    let mut scene = fixture();
    let id = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(id, 0, ObjectFrame::Surfaces(surfaces()));
    assert!(scene.geometry_bytes() > 0, "the packed geometry is counted");
    {
        let rendered = scene.rendered();
        assert_eq!(rendered.layers.len(), 1, "the surface layer renders");
        assert_eq!(rendered.styles.len(), 2, "every surface carries a style");
        let layer = rendered.layers[0];
        let mesh = layer.mesh.expect("the layer carries geometry");
        assert_eq!(mesh.triangle_count(), 4);
        assert_eq!(mesh.vertex_count(), 8);
        assert_eq!(layer.style_count, 2);
    }
    let ray = Ray {
        origin: Vec3::new(-1.0, -1.0, 4.0),
        direction: Vec3::new(0.0, 0.0, -1.0),
    };
    assert_eq!(
        scene.pick_ray(&ray),
        Some(Selection {
            set: id,
            item: Item::Object(0),
        })
    );
    scene
        .objects
        .get_mut(id)
        .expect("the surfaces exist")
        .set_visible(false);
    let rendered = scene.rendered();
    assert!(rendered.layers.is_empty(), "a hidden set renders nothing");
    assert_eq!(scene.pick_ray(&ray), None);
}

fn fixture() -> Scene {
    let channel = ChannelInfo::new(
        "signal",
        [0.3, 0.6, 1.0],
        Intensity::new(0.0, 100.0),
        Intensity::new(0.0, 100.0),
        VoxelFormat::U16,
    );
    let pyramid = Pyramid::new(
        vec![Level::new(
            Extent::new(16, 16, 16),
            Spacing::new(1.0, 1.0, 1.0),
        )],
        vec![channel],
        1,
        1.0,
    );
    Scene::new(Dataset::synthetic("test", pyramid))
}

fn surfaces() -> SurfaceSet {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, offset) in [-2.0f32, 0.0].into_iter().enumerate() {
        let base = vertices.len() as u32;
        for (x, y) in [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)] {
            vertices.push(MeshVertex::new(
                [offset + x, offset + y, 0.0],
                [0.0, 0.0, 1.0],
                object as u32,
            ));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let objects = (0..2)
        .map(|index| {
            let offset = if index == 0 { -2.0 } else { 0.0 };
            SurfaceStatistics::new(
                4,
                4.0,
                4.0,
                [offset + 1.0, offset + 1.0, 0.0],
                Bounds::new([offset, offset, 0.0], [offset + 2.0, offset + 2.0, 0.0]),
            )
        })
        .collect();
    let measured = SampleStatistics::new(1.0, 1.0, 1.0, 0.0, 4.0);
    SurfaceSet::new(
        0,
        Mesh::new(vertices, indices),
        objects,
        IntensityTable::new(1, vec![measured; 2]),
    )
}
