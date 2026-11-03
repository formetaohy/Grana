use grana_measure::{
    ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo, ColocSignificance,
};
use grana_object::{IntensityTable, RelationFrame, SampleAccumulator, Spot, SpotSet};
use grana_scene::{Dataset, Item, ObjectFrame, ObjectKind, Scene, Selection};
use grana_volume::{Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

fn scene() -> Scene {
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
    assert!(scene.add_relation(target, source));
    scene.install_relation(
        target,
        source,
        0,
        RelationFrame::new(vec![0.5], vec![0.0], vec![2]),
    );
    scene.select(Some(Selection {
        set: source,
        item: Item::Object(2),
    }));

    assert!(scene.delete_object(source, 0, 1));
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
            .and_then(|set| set.relation(source))
            .is_some_and(|relation| relation.frame(0).is_none()),
        "a deleted detection invalidates the relation it fed"
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
