use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

fn scene() -> Scene {
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

fn frame(x: f32) -> ObjectFrame {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    ObjectFrame::Spots(SpotSet::new(
        0,
        vec![Spot::new([x, 0.0, 0.0], 1.0, 0.5)],
        IntensityTable::new(1, vec![accumulator.statistics()]),
    ))
}

#[test]
fn restores_a_document_with_fresh_geometry_stamps() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, frame(1.0));
    let first = scene
        .objects
        .get(set)
        .and_then(|set| set.frame_stamp(0))
        .expect("the frame carries a stamp");
    let document = scene.document_hash();
    let state = scene.state();
    scene.install(set, 0, frame(2.0));
    let second = scene
        .objects
        .get(set)
        .and_then(|set| set.frame_stamp(0))
        .expect("the frame carries a stamp");
    assert!(second > first);
    assert_ne!(scene.document_hash(), document);
    scene.restore(state);
    assert_eq!(scene.document_hash(), document);
    let restored = scene
        .objects
        .get(set)
        .and_then(|set| set.frame_stamp(0))
        .expect("the restored frame carries a stamp");
    assert!(restored > second);
    assert_eq!(
        scene
            .objects
            .get(set)
            .and_then(|set| set.frame(0))
            .map(ObjectFrame::spots_of)
            .and_then(|spots| spots.first())
            .map(|spot| spot.center()),
        Some([1.0, 0.0, 0.0])
    );
}
