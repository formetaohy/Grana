use grana_measure::{MeasureView, Plot, PlotAxis};
use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectKind, Scene};
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
fn plots_and_gates_measurements() {
    let mut scene = scene();
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, frame());
    let mut measure = Measure::new();
    measure.select(set, &scene);
    assert_eq!(measure.view(), MeasureView::Table);
    assert!(measure.plot().is_none());

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

    let state = measure.state();
    let document = measure.document_hash();
    let mut restored = Measure::new();
    restored.restore(state, &scene);
    assert_eq!(restored.state(), measure.state());
    assert_eq!(restored.document_hash(), document);
    assert_eq!(restored.plot().map(Plot::view), Some(MeasureView::Scatter));
}
