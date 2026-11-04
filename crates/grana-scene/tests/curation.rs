use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet, Track, TrackNode, TrackSet};
use grana_scene::{Dataset, ObjectFrame, ObjectKind, ObjectSetId, Scene};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

fn scene(frames: usize) -> Scene {
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
        VolumeSeries::new(vec![volume; frames], 1.0)
            .pyramid()
            .clone(),
    ))
}

fn spots(centres: &[[f32; 3]]) -> ObjectFrame {
    sized(centres, 1.0)
}

fn sized(centres: &[[f32; 3]], radius: f32) -> ObjectFrame {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    ObjectFrame::Spots(SpotSet::new(
        0,
        centres
            .iter()
            .map(|centre| Spot::new(*centre, radius, 0.5))
            .collect::<Vec<_>>(),
        IntensityTable::new(1, vec![accumulator.statistics(); centres.len()]),
    ))
}

fn placed(scene: &Scene, set: ObjectSetId, time: u32) -> Vec<f32> {
    let frame = scene
        .objects
        .get(set)
        .and_then(|object| object.frame(time))
        .expect("the frame is detected");
    let ObjectFrame::Spots(spots) = frame else {
        panic!("the fixture holds spots");
    };
    spots.spots().iter().map(|spot| spot.center()[0]).collect()
}

fn tracks(scene: &Scene, set: ObjectSetId) -> usize {
    scene
        .objects
        .get(set)
        .and_then(|object| object.tracks())
        .map_or(0, |layer| layer.set().len())
}

#[test]
fn replays_manual_removals_on_a_rebuilt_frame() {
    let mut scene = scene(1);
    let set = scene.add_object(ObjectKind::Spots, 0);
    let centres = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    scene.install(set, 0, spots(&centres));
    assert!(
        scene
            .delete_objects(set, 0, &[1])
            .expect("the spot is pruned")
    );
    assert_eq!(placed(&scene, set, 0), [0.0, 2.0]);
    let curation = scene
        .objects
        .get(set)
        .expect("the object set exists")
        .curation();
    assert_eq!(curation.removal_count(), 1);
    assert_eq!(curation.expected(), [Some(3)]);

    scene.install(set, 0, spots(&centres));
    assert_eq!(
        scene.apply_curation(set),
        1,
        "the rebuilt frame drops the removed object again"
    );
    assert_eq!(placed(&scene, set, 0), [0.0, 2.0]);
    assert_eq!(
        scene.apply_curation(set),
        0,
        "the curation applies once per produced frame"
    );
    assert_eq!(placed(&scene, set, 0), [0.0, 2.0]);

    assert!(
        scene
            .delete_objects(set, 0, &[1])
            .expect("the second spot is pruned")
    );
    assert_eq!(
        placed(&scene, set, 0),
        [0.0],
        "a second removal maps through the one before it"
    );
    scene.install(set, 0, spots(&centres));
    assert_eq!(scene.apply_curation(set), 2);
    assert_eq!(placed(&scene, set, 0), [0.0]);
}

#[test]
fn replays_a_removal_after_the_object_order_changes() {
    let mut scene = scene(1);
    let set = scene.add_object(ObjectKind::Spots, 0);
    let centres = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    scene.install(set, 0, spots(&centres));
    assert!(
        scene
            .delete_objects(set, 0, &[1])
            .expect("the spot is pruned")
    );
    assert_eq!(placed(&scene, set, 0), [0.0, 2.0]);

    let reordered = [centres[1], centres[0], centres[2]];
    scene.install(set, 0, spots(&reordered));
    assert_eq!(
        scene.apply_curation(set),
        1,
        "the rebuilt frame drops the anchor the user removed"
    );
    assert_eq!(
        placed(&scene, set, 0),
        [0.0, 2.0],
        "the removal follows the object rather than its index"
    );
}

#[test]
fn drops_the_manual_removals_of_a_frame_that_holds_other_objects() {
    let mut scene = scene(1);
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, spots(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]));
    assert!(
        scene
            .delete_objects(set, 0, &[0])
            .expect("the spot is pruned")
    );
    assert_eq!(placed(&scene, set, 0), [1.0]);

    scene.install(
        set,
        0,
        spots(&[[4.0, 0.0, 0.0], [5.0, 0.0, 0.0], [6.0, 0.0, 0.0]]),
    );
    assert_eq!(
        scene.apply_curation(set),
        1,
        "the curation is dropped when the frame holds another object count"
    );
    assert_eq!(
        placed(&scene, set, 0),
        [4.0, 5.0, 6.0],
        "a dropped curation keeps every produced object"
    );
    let curation = scene
        .objects
        .get(set)
        .expect("the object set exists")
        .curation();
    assert_eq!(curation.removal_count(), 0);
    assert_eq!(curation.dropped(), 1);
}

#[test]
fn replays_a_removal_after_the_object_splits() {
    let mut scene = scene(1);
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, sized(&[[0.0, 0.0, 0.0]], 2.0));
    assert!(
        scene
            .delete_objects(set, 0, &[0])
            .expect("the large spot is pruned")
    );

    scene.install(set, 0, spots(&[[-1.5, 0.0, 0.0], [0.9, 0.0, 0.0]]));
    assert_eq!(
        scene.apply_curation(set),
        1,
        "the piece that keeps the removed centre is dropped after a split"
    );
    assert_eq!(
        placed(&scene, set, 0),
        [-1.5],
        "the farther piece of the split survives"
    );
    assert_eq!(
        scene
            .objects
            .get(set)
            .expect("the object set exists")
            .curation()
            .dropped(),
        0,
        "the removal of a split object is never dropped"
    );
}

#[test]
fn replays_a_removal_after_the_object_merges() {
    let mut scene = scene(1);
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, spots(&[[0.0, 0.0, 0.0]]));
    assert!(
        scene
            .delete_objects(set, 0, &[0])
            .expect("the spot is pruned")
    );

    scene.install(set, 0, sized(&[[0.9, 0.0, 0.0]], 2.0));
    assert_eq!(
        scene.apply_curation(set),
        1,
        "the object that swallowed the removed one is dropped after a merge"
    );
    assert!(
        scene
            .objects
            .get(set)
            .expect("the object set exists")
            .frame(0)
            .is_none(),
        "the merged object leaves the rebuilt frame"
    );
    assert_eq!(
        scene
            .objects
            .get(set)
            .expect("the object set exists")
            .curation()
            .dropped(),
        0,
        "the removal of a merged object is never dropped"
    );
}

#[test]
fn replays_track_edits_after_a_retrack() {
    let mut scene = scene(2);
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(
            set,
            time,
            spots(&[[time as f32, 0.0, 0.0], [time as f32, 1.0, 0.0]]),
        );
    }
    let chain =
        |time: u32, object: usize| TrackNode::new(time, object, [time as f32, object as f32, 0.0]);
    let detections = TrackSet::new(
        1.0,
        vec![
            Track::new(None, vec![chain(0, 0), chain(1, 0)]),
            Track::new(None, vec![chain(0, 1), chain(1, 1)]),
        ],
    );
    assert!(scene.objects.install_tracks(set, detections.clone()));
    assert_eq!(tracks(&scene, set), 2);
    assert!(scene.cut_track(set, 0, 0));
    assert_eq!(tracks(&scene, set), 3, "the cut splits one track in two");
    assert_eq!(
        scene
            .objects
            .get(set)
            .expect("the object set exists")
            .curation()
            .edit_count(),
        1
    );

    assert!(scene.objects.install_tracks(set, detections));
    assert_eq!(
        tracks(&scene, set),
        3,
        "a re-track replays the cut of the curation"
    );
}
