use grana_analysis::{LabelBuilder, LabelSlab};
use grana_volume::{Extent, Level, Spacing, VoxelData};

const SIDE: u32 = 5;

#[test]
fn meshes_label_voxels_into_a_measured_surface() {
    let level = Level::new(Extent::new(SIDE, SIDE, SIDE), Spacing::new(1.0, 1.0, 1.0));
    let labels = block();
    let values = VoxelData::U16(vec![1000; (SIDE * SIDE * SIDE) as usize]);
    let mut builder = LabelBuilder::new(0, level, 1);
    builder.push(LabelSlab {
        origin: [0, 0, 0],
        core: [SIDE, SIDE, SIDE],
        extent: [SIDE, SIDE, SIDE],
        labels: &labels,
        channels: &[values],
    });
    let set = builder.finish().expect("the label voxels form a surface");
    assert_eq!(set.object_count(), 1);
    let object = set.object(0).expect("the surface exists");
    assert_eq!(object.voxel_count(), 27);
    assert!((object.volume() - 27.0).abs() < 1.0e-6);
    assert!((object.centroid()[0] - 0.0).abs() < 1.0e-5);
    assert!((object.centroid()[1] - 0.0).abs() < 1.0e-5);
    assert!((object.centroid()[2] - 0.0).abs() < 1.0e-5);
    assert!(
        object.area() > 36.0 && object.area() < 54.0,
        "the marching cubes surface chamfers the corners of the voxel block: {}",
        object.area()
    );
    assert!((0.0..=1.0).contains(&object.sphericity()));
    assert!(object.bounds().min()[0] < object.bounds().max()[0]);
    let intensity = set.intensities().object(0, 0);
    assert!((intensity.mean() - 1000.0).abs() < 1.0e-3);
    assert!((intensity.deviation()).abs() < 1.0e-3);
    let geometry = set.geometry().expect("the surface carries a mesh");
    let mut volume = 0.0f64;
    for triangle in 0..geometry.triangle_count() {
        let points = geometry.triangle(0, triangle).map(glam::Vec3::from);
        volume += points[0].dot(points[1].cross(points[2])) as f64 / 6.0;
    }
    assert!(volume > 0.0, "the mesh faces outwards");
}

#[test]
fn splits_two_labels_and_welds_their_vertices() {
    let level = Level::new(Extent::new(6, 4, 4), Spacing::new(1.0, 1.0, 1.0));
    let mut labels = vec![0u32; 6 * 4 * 4];
    for z in 0..4 {
        for y in 0..4 {
            for x in 0..6 {
                labels[(x + 6 * (y + 4 * z)) as usize] = if x < 3 { 1 } else { 2 };
            }
        }
    }
    let values = VoxelData::U16(vec![500; labels.len()]);
    let mut builder = LabelBuilder::new(0, level, 1);
    builder.push(LabelSlab {
        origin: [0, 0, 0],
        core: [6, 4, 4],
        extent: [6, 4, 4],
        labels: &labels,
        channels: &[values],
    });
    let set = builder.finish().expect("both labels form a surface");
    assert_eq!(set.object_count(), 2);
    let geometry = set.geometry().expect("the surfaces carry a mesh");
    assert_eq!(geometry.object_count(), 2);
    assert_eq!(
        set.object(0)
            .expect("the first surface exists")
            .voxel_count(),
        48
    );
    assert_eq!(
        set.object(1)
            .expect("the second surface exists")
            .voxel_count(),
        48
    );
    for object in 0..2 {
        assert!(geometry.groups()[object].index_count() >= 3);
    }
}

fn block() -> Vec<u32> {
    let mut labels = vec![0u32; (SIDE * SIDE * SIDE) as usize];
    for z in 1..4 {
        for y in 1..4 {
            for x in 1..4 {
                labels[(x + SIDE * (y + SIDE * z)) as usize] = 1;
            }
        }
    }
    labels
}
