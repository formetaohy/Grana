use grana_measure::{
    Aggregate, ChildField, ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo,
    ColocSignificance, Group, RelationField,
};
use grana_object::{
    IntensityTable, RelationFrame, SampleAccumulator, Spot, SpotSet, Track, TrackNode, TrackSet,
};
use grana_scene::{
    Criteria, Dataset, FilterRange, Item, ObjectColor, ObjectFrame, ObjectKind, Scene, Selection,
    TrackColor,
};
use grana_volume::{Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

fn series(frames: usize) -> Scene {
    let extent = Extent::new(4, 4, 2);
    let volume = Volume::new(
        extent,
        Spacing::new(0.5, 0.5, 1.0),
        vec![
            Channel::new(
                "signal",
                [1.0, 1.0, 1.0],
                VoxelData::U16(vec![1; extent.voxel_count() as usize]),
            ),
            Channel::new(
                "second",
                [1.0, 0.5, 0.5],
                VoxelData::U16(vec![1; extent.voxel_count() as usize]),
            ),
        ],
    );
    Scene::new(Dataset::synthetic(
        "fixture",
        VolumeSeries::new(vec![volume; frames], 1.0)
            .pyramid()
            .clone(),
    ))
}

fn scene() -> Scene {
    series(1)
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

fn spots(centres: &[[f32; 3]]) -> ObjectFrame {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    ObjectFrame::Spots(SpotSet::new(
        0,
        centres
            .iter()
            .map(|centre| Spot::new(*centre, 1.0, 0.5))
            .collect(),
        IntensityTable::new(1, vec![accumulator.statistics(); centres.len()]),
    ))
}

fn coloc_series() -> ColocSeries {
    let bins = 32;
    let mut frame = ColocFrame::new(bins);
    frame.push(0.25, 0.75);
    frame.push(0.5, 0.5);
    let mut joint = vec![0; bins * bins];
    joint[3 + bins * 9] = 2;
    ColocSeries::from_parts(
        ColocPair::new(0, 1),
        ColocSeriesInfo::new(0, 1, 1, None),
        bins,
        vec![frame],
        joint,
        Some(ColocSignificance::new(0.004_975, 200, 1.0)),
        ColocCost::new(2, 64, 1, 200, 4),
    )
}

#[test]
fn renames_and_deletes_a_detection_without_losing_identities() {
    let mut scene = scene();
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    let reserved = scene
        .objects
        .get(target)
        .expect("the target set exists")
        .name()
        .to_string();

    assert!(scene.rename_object(source, "Cells"));
    assert_eq!(
        scene.objects.get(source).map(|set| set.name()),
        Some("Cells")
    );
    assert!(
        !scene.rename_object(target, "Cells"),
        "an object set name is unique"
    );
    assert!(
        !scene.rename_object(target, "  "),
        "an object set name is spoken"
    );
    assert!(
        !scene.rename_object(target, "cells"),
        "an object set name does not collide with an output name"
    );
    assert_eq!(
        scene.objects.get(target).map(|set| set.name()),
        Some(reserved.as_str())
    );

    scene.install(
        source,
        0,
        spots(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
    );
    scene.install(target, 0, spots(&[[0.0, 0.0, 0.0]]));
    assert!(scene.add_relation(target, source, 0));
    scene.install_relation(
        target,
        source,
        0,
        0,
        RelationFrame::new(vec![0.5], vec![0.0], vec![0.0], vec![2]),
    );
    scene.select(Some(Selection {
        set: source,
        item: Item::Object(2),
    }));

    assert!(
        scene
            .delete_object(source, 0, 1)
            .expect("the object is deleted")
    );
    assert_eq!(
        scene
            .objects
            .get(source)
            .and_then(|set| set.frame(0))
            .map(ObjectFrame::object_count),
        Some(2)
    );
    assert_eq!(
        scene.selection(),
        Some(Selection {
            set: source,
            item: Item::Object(1),
        }),
        "the selection follows the surviving identity"
    );
    assert!(
        scene
            .objects
            .get(target)
            .and_then(|set| set.relation(source, 0))
            .is_some_and(|relation| relation.frame(0).is_none()),
        "a deleted detection invalidates the relation it fed"
    );
}

#[test]
fn colors_objects_by_their_relation_to_another_set() {
    let mut scene = scene();
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        source,
        0,
        spots(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [4.0, 0.0, 0.0]]),
    );
    scene.install(target, 0, spots(&[[0.0, 0.0, 0.0]]));
    assert!(scene.add_relation(source, target, 0));
    let set = scene.objects.get(source).expect("the source set exists");
    let mut coloring = set.object_coloring().clone();
    coloring.set_metric(Some(ObjectColor::Relation {
        target,
        field: RelationField::Distance,
        shift: 0,
    }));
    assert!(scene.objects.set_object_coloring(source, coloring));
    assert!(
        scene
            .objects
            .get(source)
            .expect("the source set exists")
            .object_extent()
            .is_none(),
        "a relation that measured nothing colors no object"
    );

    scene.install_relation(
        source,
        target,
        0,
        0,
        RelationFrame::new(
            vec![0.5, 0.25, 0.75],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 0.0],
            vec![1, 1, 1],
        ),
    );
    assert_eq!(
        scene
            .objects
            .get(source)
            .expect("the source set exists")
            .object_extent(),
        Some((0.25, 0.75)),
        "the relation colors spread over the measured distances"
    );

    assert!(scene.remove_relation(source, target, 0));
    assert!(
        scene
            .objects
            .get(source)
            .expect("the source set exists")
            .object_coloring()
            .metric()
            .is_none(),
        "removing the relation falls back to the set color"
    );
}

#[test]
fn shifts_a_relation_in_time_and_trims_its_frames() {
    let mut scene = series(3);
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..3 {
        scene.install(source, time, spots(&[[0.0, 0.0, 0.0]]));
        scene.install(target, time, spots(&[[1.0, 0.0, 0.0]]));
    }
    assert!(scene.add_relation(source, target, -1));
    assert_eq!(
        scene
            .objects
            .get(source)
            .and_then(|set| set.relation(target, -1))
            .map(|relation| relation.shift()),
        Some(-1)
    );
    scene.install_relation(
        source,
        target,
        -1,
        2,
        RelationFrame::new(vec![2.0], vec![0.0], vec![0.0], vec![1]),
    );
    scene.install(target, 1, spots(&[[3.0, 0.0, 0.0]]));
    assert!(
        scene
            .objects
            .get(source)
            .and_then(|set| set.relation(target, -1))
            .is_some_and(|relation| relation.frame(2).is_none()),
        "a changed target frame invalidates the relation that measured it"
    );

    scene.install_relation(
        source,
        target,
        -1,
        2,
        RelationFrame::new(vec![2.0], vec![0.0], vec![0.0], vec![1]),
    );
    assert!(scene.move_relation(source, target, -1, 0));
    assert!(
        scene
            .objects
            .get(source)
            .and_then(|set| set.relation(target, 0))
            .is_some_and(|relation| relation.computed() == 0),
        "changing the shift unmeasures the relation"
    );
    assert!(
        !scene.add_relation(source, source, 0),
        "a relation to the same frame of itself measures nothing"
    );
    assert!(scene.add_relation(source, source, -2));
    assert!(
        !scene.move_relation(source, target, 0, 3),
        "a shift outside the series is refused"
    );
}

#[test]
fn colors_tracks_by_a_reduced_relation() {
    let mut scene = series(2);
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(source, time, spots(&[[0.0, 0.0, 0.0]]));
        scene.install(target, time, spots(&[[1.0, 0.0, 0.0]]));
    }
    assert!(scene.add_relation(source, target, 0));
    for (time, reach) in [(0, 0.0), (1, 6.0)] {
        scene.install_relation(
            source,
            target,
            0,
            time,
            RelationFrame::new(vec![reach], vec![0.0], vec![0.0], vec![1]),
        );
    }
    let centre = |time: u32| TrackNode::new(time, 0, [time as f32, 0.0, 0.0]);
    assert!(scene.objects.install_tracks(
        source,
        TrackSet::new(1.0, vec![Track::new(None, vec![centre(0), centre(1)])]),
    ));
    let set = scene
        .objects
        .get_mut(source)
        .expect("the source set exists");
    let mut coloring = set.track_coloring().clone();
    coloring.set_metric(Some(TrackColor::Relation {
        target,
        field: RelationField::Distance,
        aggregate: Aggregate::Maximum,
        shift: 0,
    }));
    assert!(set.set_track_coloring(coloring));
    assert_eq!(
        scene
            .objects
            .get(source)
            .expect("the source set exists")
            .track_extent(),
        Some((6.0, 6.0)),
        "a track takes the maximum of the distances it measured"
    );
    let set = scene
        .objects
        .get_mut(source)
        .expect("the source set exists");
    let mut coloring = set.track_coloring().clone();
    coloring.set_metric(Some(TrackColor::Relation {
        target,
        field: RelationField::Distance,
        aggregate: Aggregate::Mean,
        shift: 0,
    }));
    assert!(set.set_track_coloring(coloring));
    assert_eq!(
        scene
            .objects
            .get(source)
            .expect("the source set exists")
            .track_extent(),
        Some((3.0, 3.0)),
        "the reducer of a track color is explicit"
    );
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

#[test]
fn invalidates_colocation_results_when_the_inputs_change() {
    let mut scene = scene();
    scene.coloc.install(coloc_series());
    let document = scene.document_hash();
    let state = scene.state();
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, frame(0.5));
    scene.set_mask(Some(set));
    assert!(
        scene.coloc.results().is_empty(),
        "the mask change invalidates the colocalization result"
    );
    assert_ne!(scene.document_hash(), document);
    scene.restore(state);
    assert_eq!(
        scene.coloc.results().len(),
        1,
        "the restored state keeps the colocalization result"
    );

    let state = scene.state();
    scene.set_region(Region::new(
        glam::Vec3::new(-1.0, -1.0, -1.0),
        glam::Vec3::new(1.0, 1.0, 1.0),
    ));
    assert!(
        scene.coloc.results().is_empty(),
        "the region change invalidates the colocalization result"
    );
    assert_ne!(scene.document_hash(), document);
    scene.restore(state);
    assert_eq!(scene.coloc.results().len(), 1);
}

fn pair_spots(centres: &[[f32; 3]]) -> ObjectFrame {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    let statistics = accumulator.statistics();
    ObjectFrame::Spots(SpotSet::new(
        0,
        centres
            .iter()
            .map(|centre| Spot::new(*centre, 1.0, 0.5))
            .collect(),
        IntensityTable::new(2, vec![statistics; 2 * centres.len()]),
    ))
}
#[test]
fn measures_one_relation_at_several_offsets() {
    let mut scene = series(3);
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..3 {
        scene.install(source, time, pair_spots(&[[0.0, 0.0, 0.0]]));
        scene.install(target, time, pair_spots(&[[1.0, 0.0, 0.0]]));
    }
    assert!(scene.add_relation(source, target, 0));
    assert!(scene.add_relation(source, target, -1));
    assert!(
        !scene.add_relation(source, target, 0),
        "an offset is measured once per target"
    );
    scene.install_relation(
        source,
        target,
        0,
        0,
        RelationFrame::new(vec![1.0], vec![0.0], vec![0.0], vec![1]),
    );
    scene.install_relation(
        source,
        target,
        -1,
        2,
        RelationFrame::new(vec![2.0], vec![0.0], vec![0.0], vec![1]),
    );
    let table = scene
        .table(source, Group::Object, Aggregate::Mean)
        .expect("the source set measures its frames");
    assert!(
        table.column("r2_0_distance_um").is_some(),
        "the same frame offset keeps the plain column"
    );
    assert!(
        table.column("r2_-1_distance_um").is_some(),
        "a shifted offset names its own column"
    );
    assert!(scene.remove_relation(source, target, 0));
    let set = scene.objects.get(source).expect("the source set exists");
    assert!(
        set.relation(target, -1).is_some(),
        "the other offset survives the removal"
    );
    assert!(set.relation(target, 0).is_none());
}

fn two_channel_spots(centres: &[[f32; 3]]) -> ObjectFrame {
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    ObjectFrame::Spots(SpotSet::new(
        0,
        centres
            .iter()
            .map(|centre| Spot::new(*centre, 1.0, 0.5))
            .collect(),
        IntensityTable::new(2, vec![accumulator.statistics(); centres.len() * 2]),
    ))
}

#[test]
fn aggregates_children_into_their_parent_objects() {
    let mut scene = scene();
    let parent = scene.add_object(ObjectKind::Spots, 0);
    let child = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        parent,
        0,
        two_channel_spots(&[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]]),
    );
    scene.install(
        child,
        0,
        two_channel_spots(&[[0.2, 0.0, 0.0], [4.2, 0.0, 0.0], [4.4, 0.0, 0.0]]),
    );
    assert!(scene.add_relation(child, parent, 0));
    scene.install_relation(
        child,
        parent,
        0,
        0,
        RelationFrame::new(
            vec![0.0, 0.0, 1.0],
            vec![1.0, 1.0, 0.0],
            vec![0.0, 0.0, 0.0],
            vec![1, 1, 2],
        ),
    );
    let child_key = scene
        .objects
        .get(child)
        .expect("the child set exists")
        .key()
        .as_str()
        .to_string();
    let table = scene
        .table(parent, Group::Object, Aggregate::Mean)
        .expect("the parent measures its children");
    let count = table
        .column(&format!("{child_key}_count"))
        .expect("the child count column exists");
    assert_eq!(table.value(0, count), 2.0);
    assert_eq!(table.value(1, count), 0.0);
    let diameter = table
        .column(&format!("{child_key}_diameter_um_mean"))
        .expect("the child diameter column exists");
    assert_eq!(table.value(0, diameter), 2.0);
    assert!(table.value(1, diameter).is_nan());

    let mut coloring = scene
        .objects
        .get(parent)
        .expect("the parent set exists")
        .object_coloring()
        .clone();
    coloring.set_metric(Some(ObjectColor::Child {
        target: child,
        field: ChildField::Count,
    }));
    assert!(scene.objects.set_object_coloring(parent, coloring));
    let set = scene.objects.get(parent).expect("the parent set exists");
    assert_eq!(set.object_extent(), Some((0.0, 2.0)));
    assert_eq!(
        scene
            .object_colors(set)
            .expect("the parents are colored")
            .len(),
        2
    );

    let criteria = Criteria::new(
        Group::Object,
        vec![FilterRange::new(&format!("{child_key}_count"), 1.0, 2.0)],
        None,
    );
    let filtered = scene
        .filter_object(parent, "counted", criteria)
        .expect("the derived set exists");
    assert!(scene.refresh_filtered(filtered).expect("the filter runs"));
    assert_eq!(
        scene
            .objects
            .get(filtered)
            .and_then(|set| set.frame(0))
            .map_or(0, ObjectFrame::object_count),
        1,
        "only the parent that holds children survives the filter"
    );
}
