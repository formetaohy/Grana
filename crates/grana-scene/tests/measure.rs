use std::sync::Arc;

use grana_measure::{Lasso, MeasureView, Plot, PlotAxis, Scope};
use grana_object::{IntensityTable, RelationFrame, SampleAccumulator, Spot, SpotSet};
use grana_scene::{
    Dataset, MaskFilter, Measure, ObjectFrame, ObjectKind, RelationField, Scene, Visibility,
};
use grana_volume::{Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

fn scene() -> Scene {
    let extent = Extent::new(4, 4, 2);
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

#[test]
fn detects_inside_the_filtered_objects_of_the_mask_set() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    let centered = || {
        let statistics = [0.1f32, 0.9].map(|value| {
            let mut accumulator = SampleAccumulator::new();
            accumulator.push(value);
            accumulator.statistics()
        });
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![
                Spot::new([-0.4, 0.0, 0.0], 0.6, 0.5),
                Spot::new([0.4, 0.0, 0.0], 0.6, 0.5),
            ],
            IntensityTable::new(1, statistics.to_vec()),
        ))
    };
    for time in 0..2 {
        scene.install(set, time, centered());
    }
    scene.set_mask(Some(set));
    let level = scene.dataset.pyramid().level(0);
    let whole = scene
        .mask_frames()
        .expect("the mask set builds")
        .expect("the mask names a set")
        .mask(level, 0)
        .expect("the whole mask rasterizes");
    assert!(whole.count() > 0, "the mask rasterizes the spot set");
    scene
        .objects
        .get_mut(set)
        .expect("the mask set exists")
        .set_visibility(Visibility::Object(Arc::new(vec![
            vec![false, true],
            vec![false, true],
        ])));
    let filtered = scene
        .mask_frames()
        .expect("the filtered mask set builds")
        .expect("the mask names a set")
        .mask(level, 0)
        .expect("the filtered mask rasterizes");
    assert!(
        filtered.count() > 0 && filtered.count() < whole.count(),
        "the analysis mask follows the filter visibility"
    );
}

#[test]
fn limits_the_analysis_mask_to_a_relation_range() {
    let mut scene = scene();
    let source = scene.add_object(ObjectKind::Spots, 0);
    let target = scene.add_object(ObjectKind::Spots, 0);
    let statistics = |count: usize| {
        (0..count)
            .map(|_| {
                let mut accumulator = SampleAccumulator::new();
                accumulator.push(0.5);
                accumulator.statistics()
            })
            .collect::<Vec<_>>()
    };
    for time in 0..2 {
        scene.install(
            source,
            time,
            ObjectFrame::Spots(SpotSet::new(
                0,
                vec![
                    Spot::new([-0.4, 0.0, 0.0], 0.6, 0.5),
                    Spot::new([0.4, 0.0, 0.0], 0.6, 0.5),
                ],
                IntensityTable::new(1, statistics(2)),
            )),
        );
        scene.install(
            target,
            time,
            ObjectFrame::Spots(SpotSet::new(
                0,
                vec![Spot::new([0.0, 0.0, 0.0], 0.3, 0.5)],
                IntensityTable::new(1, statistics(1)),
            )),
        );
    }
    scene.set_mask(Some(source));
    let level = scene.dataset.pyramid().level(0);
    let whole = scene
        .mask_frames()
        .expect("the mask set builds")
        .expect("the mask names a set")
        .mask(level, 0)
        .expect("the whole mask rasterizes");
    assert!(scene.add_relation(source, target, 0));
    let filter = MaskFilter::new(target, 0, RelationField::Distance, 0.0, 1.0);
    assert!(scene.set_mask_filter(Some(filter)));
    let missing = match scene.mask_frames() {
        Err(detail) => detail,
        Ok(_) => panic!("the relation is not measured yet"),
    };
    assert!(missing.contains("needs the relation"));
    for time in 0..2 {
        scene.install_relation(
            source,
            target,
            0,
            time,
            RelationFrame::new(vec![0.2, 20.0], vec![0.0, 0.0], vec![0.0, 0.0], vec![1, 1]),
        );
    }
    let limited = scene
        .mask_frames()
        .expect("the filtered mask set builds")
        .expect("the mask names a set")
        .mask(level, 0)
        .expect("the filtered mask rasterizes");
    assert!(
        limited.count() > 0 && limited.count() < whole.count(),
        "the analysis mask keeps the objects inside the relation range"
    );
}

fn frame() -> ObjectFrame {
    let statistics = [0.5f32, 0.6, 0.7].map(|value| {
        let mut accumulator = SampleAccumulator::new();
        accumulator.push(value);
        accumulator.statistics()
    });
    ObjectFrame::Spots(SpotSet::new(
        0,
        vec![
            Spot::new([1.0, 0.0, 0.0], 0.5, 0.2),
            Spot::new([2.0, 0.0, 0.0], 1.0, 0.4),
            Spot::new([3.0, 0.0, 0.0], 1.5, 0.6),
        ],
        IntensityTable::new(1, statistics.to_vec()),
    ))
}

#[test]
fn scopes_the_table_to_the_statistics_region() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, frame());
    }
    let mut measure = Measure::new();
    measure.select(set, &scene);
    assert_eq!(measure.total(), 6);
    measure.set_roi(Some(Region::new(
        glam::Vec3::new(0.6, -0.4, -0.4),
        glam::Vec3::new(1.4, 0.4, 0.4),
    )));
    assert_eq!(measure.rows().len(), 2);
    assert_eq!(measure.total(), 2);
    measure.set_roi(None);
    assert_eq!(measure.rows().len(), 6);
    assert_eq!(measure.total(), 6);
}

#[test]
fn plots_gates_lassos_and_scopes_measurements() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, frame());
    }
    let mut measure = Measure::new();
    measure.select(set, &scene);
    assert_eq!(measure.view(), MeasureView::Table);
    assert!(measure.plot().is_none());
    assert_eq!(measure.total(), 6);

    measure.set_view(MeasureView::Histogram);
    let (column, minimum, maximum, expected) = {
        let Plot::Histogram(histogram) = measure.plot().expect("a histogram builds") else {
            panic!("the histogram view builds a histogram");
        };
        let column = histogram.column();
        let table = measure.table().expect("the set measures");
        assert!(
            column >= table.identity_columns(),
            "an axis names a measurement column"
        );
        let (low, high) = table.extent(column).expect("the column holds values");
        let span = high - low;
        let minimum = low + 0.34 * span;
        let maximum = low + 0.67 * span;
        let expected = (0..table.rows())
            .filter(|row| {
                let value = table.value(*row, column);
                value >= minimum && value <= maximum
            })
            .count();
        (column, minimum, maximum, expected)
    };

    measure.set_gate(column, minimum, maximum);
    assert_eq!(measure.filters().len(), 1);
    assert_eq!(measure.rows().len(), expected);
    {
        let Plot::Histogram(histogram) = measure.plot().expect("the gate keeps the plot") else {
            panic!("the histogram view builds a histogram");
        };
        assert_eq!(histogram.summary().count() as usize, expected);
    }

    measure.set_gate(column, minimum, maximum);
    assert_eq!(
        measure.filters().len(),
        1,
        "a gate retargets its own column"
    );

    measure.set_view(MeasureView::Scatter);
    let second = {
        let Plot::Scatter(scatter) = measure.plot().expect("a scatter builds") else {
            panic!("the scatter view builds a scatter");
        };
        assert_ne!(scatter.x(), scatter.y(), "a scatter plots two columns");
        scatter.y()
    };
    let (low, high) = {
        let table = measure.table().expect("the set measures");
        table
            .extent(second)
            .expect("the second column holds values")
    };
    measure.set_gate(second, low, high);
    assert_eq!(measure.filters().len(), 2);

    measure.set_axis(PlotAxis::Y, second);
    assert_eq!(measure.axes()[1], second);

    let [x, y] = measure.axes();
    let points = {
        let table = measure.table().expect("the set measures");
        let (x_low, x_high) = table.extent(x).expect("the x column holds values");
        let (y_low, y_high) = table.extent(y).expect("the y column holds values");
        vec![
            [x_low, y_low],
            [x_high, y_low],
            [x_high, y_high],
            [x_low, y_high],
        ]
    };
    let gated = measure.rows().to_vec();
    let lasso = Lasso::new(x, y, points).expect("the polygon covers the plot");
    measure.set_lasso(Some(lasso));
    assert!(measure.lasso().is_some());
    assert_eq!(
        measure.rows().len(),
        gated.len(),
        "a lasso over the whole extent keeps every gated row"
    );

    let (expected_total, expected_rows) = {
        let table = measure.table().expect("the set measures");
        let time = scene.time();
        (
            (0..table.rows())
                .filter(|row| table.observation(*row).time() == Some(time))
                .count(),
            gated
                .iter()
                .filter(|row| table.observation(**row).time() == Some(time))
                .count(),
        )
    };
    measure.set_scope(Scope::Current);
    assert_eq!(measure.total(), expected_total);
    assert_eq!(measure.rows().len(), expected_rows);

    scene.set_time(1);
    measure.sync(&scene, false);
    assert_eq!(
        measure.rows().len(),
        expected_rows,
        "every frame measures the same objects"
    );
    assert_eq!(measure.total(), expected_total);
    assert_eq!(
        measure.filters().len(),
        2,
        "the gates survive a frame change"
    );
    assert!(
        measure.lasso().is_some(),
        "the lasso survives a frame change"
    );

    let state = measure.state();
    let document = measure.document_hash();
    let mut restored = Measure::new();
    restored.restore(state, &scene);
    assert_eq!(restored.state(), measure.state());
    assert_eq!(restored.document_hash(), document);
    assert_eq!(restored.plot().map(Plot::view), Some(MeasureView::Scatter));

    let other = (0..measure.table().expect("the set measures").columns().len())
        .find(|candidate| *candidate != x && *candidate != y)
        .expect("another column exists");
    measure.set_axis(PlotAxis::X, other);
    assert!(
        measure.lasso().is_none(),
        "the lasso follows the plotted axes"
    );
}
