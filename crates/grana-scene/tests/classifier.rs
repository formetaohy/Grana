use grana_measure::{Aggregate, Filter, Group, Measurement, SCORE_KEY};
use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_scene::{Dataset, ObjectColor, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

fn scene() -> Scene {
    let extent = Extent::new(8, 8, 4);
    let frames = (0..2)
        .map(|_| {
            Volume::new(
                extent,
                Spacing::new(0.5, 0.5, 1.0),
                vec![Channel::new(
                    "signal",
                    [1.0, 1.0, 1.0],
                    VoxelData::U16(vec![1; extent.voxel_count() as usize]),
                )],
            )
        })
        .collect();
    Scene::new(Dataset::synthetic(
        "fixture",
        VolumeSeries::new(frames, 1.0).pyramid().clone(),
    ))
}

fn spots(time: u32) -> ObjectFrame {
    let statistics = [0.4f32, 0.25]
        .into_iter()
        .map(|value| {
            let mut accumulator = SampleAccumulator::new();
            accumulator.push(value);
            accumulator.statistics()
        })
        .collect::<Vec<_>>();
    ObjectFrame::Spots(SpotSet::new(
        0,
        vec![
            Spot::new([time as f32, 0.0, 0.0], 1.5, 0.4),
            Spot::new([2.0, time as f32, 1.0], 0.75, 0.25),
        ],
        IntensityTable::new(1, statistics),
    ))
}

#[test]
fn trains_a_classifier_from_the_gated_rows() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, spots(time));
    }
    let table = scene
        .table(set, Group::Object, Aggregate::Mean)
        .expect("the spots build a table");
    let contrast = table
        .column("contrast")
        .expect("the table measures contrast");
    let positives = (0..table.rows())
        .filter(|row| table.value(*row, contrast) > 0.3)
        .collect::<Vec<_>>();
    assert_eq!(positives.len(), 2, "the gate keeps one spot per frame");
    scene
        .train_classifier(
            set,
            &[Measurement::Diameter, Measurement::Contrast],
            &positives,
        )
        .expect("the classifier trains on the gate");
    let classifier = scene
        .objects
        .get(set)
        .and_then(|set| set.classifier())
        .expect("the set keeps its classifier");
    assert_eq!(classifier.features().len(), 2);
    assert_eq!(classifier.positives(), 2);
    assert!(
        classifier.accuracy() > 0.99,
        "the features separate the gate, got {}",
        classifier.accuracy()
    );
    let table = scene
        .table(set, Group::Object, Aggregate::Mean)
        .expect("the classified spots build a table");
    let score = table.column(SCORE_KEY).expect("the table scores objects");
    assert_eq!(score, table.columns().len() - 1, "the score ends the table");
    for row in 0..table.rows() {
        let positive = table.value(row, contrast) > 0.3;
        let value = table.value(row, score);
        assert!(
            (value > 0.5) == positive,
            "the score {value} separates the gate at row {row}"
        );
    }
    let matching = table.matching(&[Filter::new(score, 0.5, 1.0)], None, None);
    assert_eq!(matching.len(), 2, "the score filters the gate");
}

#[test]
fn colors_and_clears_the_classified_objects() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, spots(time));
    }
    let table = scene
        .table(set, Group::Object, Aggregate::Mean)
        .expect("the spots build a table");
    let contrast = table
        .column("contrast")
        .expect("the table measures contrast");
    let positives = (0..table.rows())
        .filter(|row| table.value(*row, contrast) > 0.3)
        .collect::<Vec<_>>();
    scene
        .train_classifier(set, &[Measurement::Diameter], &positives)
        .expect("the classifier trains on the gate");
    let mut coloring = grana_display::Coloring::new();
    coloring.set_metric(Some(ObjectColor::Score));
    assert!(scene.set_object_coloring(set, coloring));
    let colors = scene
        .object_colors(scene.objects.get(set).expect("the spots exist"))
        .expect("the classified set colors its objects");
    assert_eq!(colors.len(), 2);
    assert!(scene.clear_classifier(set));
    let set = scene.objects.get(set).expect("the spots exist");
    assert!(set.classifier().is_none());
    assert!(
        set.object_coloring().metric().is_none(),
        "clearing the classifier clears its coloring"
    );
}

#[test]
fn aggregates_the_score_across_frames() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, spots(time));
    }
    let table = scene
        .table(set, Group::Object, Aggregate::Mean)
        .expect("the spots build a table");
    let contrast = table
        .column("contrast")
        .expect("the table measures contrast");
    let positives = (0..table.rows())
        .filter(|row| table.value(*row, contrast) > 0.3)
        .collect::<Vec<_>>();
    scene
        .train_classifier(set, &[Measurement::Diameter], &positives)
        .expect("the classifier trains on the gate");
    let table = scene
        .table(set, Group::Timepoint, Aggregate::Mean)
        .expect("the frames build a table");
    let score = table.column(SCORE_KEY).expect("the frames carry a score");
    assert_eq!(table.rows(), 2);
    for row in 0..table.rows() {
        let value = table.value(row, score);
        assert!(
            value.is_finite() && (0.0..=1.0).contains(&value),
            "the frame score {value} stays a probability"
        );
    }
}

#[test]
fn rejects_a_gate_without_both_classes() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, spots(time));
    }
    let error = scene
        .train_classifier(set, &[Measurement::Diameter], &[0])
        .expect_err("one kept object cannot train a classifier");
    assert!(
        error.contains("at least 2 kept"),
        "unexpected detail {error}"
    );
}
