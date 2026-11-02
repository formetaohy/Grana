use grana_measure::{Frame, Group, MeasureView, Objects, Plot, Table};
use grana_object::{SampleStatistics, Spot, SpotSet};

#[test]
fn bins_histograms_and_scatters_over_gated_rows() {
    let first = spots([0.5, 1.0], [0.1, 0.2]);
    let second = spots([1.5, 2.0], [0.3, 0.4]);
    let channels = ["EGFP".to_string()];
    let table = Table::build(
        &[
            Frame::new(0, Objects::Spots(&first)),
            Frame::new(1, Objects::Spots(&second)),
        ],
        None,
        &channels,
        1.0,
        Group::Object,
        grana_measure::Aggregate::Mean,
    );
    let diameter = column(&table, "diameter_um");
    let contrast = column(&table, "contrast");
    let gated = [0, 2];

    let Plot::Histogram(histogram) = table
        .plot(MeasureView::Histogram, [diameter, contrast], &gated)
        .expect("a histogram builds")
    else {
        panic!("a histogram mode builds a histogram");
    };
    assert_eq!(histogram.low(), 1.0);
    assert_eq!(histogram.high(), 4.0);
    assert_eq!(histogram.bin_range(0), (1.0, 1.0 + 3.0 / 48.0));
    assert_eq!(
        (0..histogram.bins())
            .map(|bin| histogram.total(bin))
            .sum::<u64>(),
        4
    );
    assert_eq!(
        (0..histogram.bins())
            .map(|bin| histogram.gated(bin))
            .sum::<u64>(),
        2
    );
    assert_eq!(histogram.peak(), 1);
    assert_eq!(histogram.summary().count(), 2);
    assert!((histogram.summary().mean() - 2.0).abs() < 1.0e-6);
    assert!((histogram.summary().deviation() - 1.0).abs() < 1.0e-6);
    assert_eq!(histogram.summary().minimum(), 1.0);
    assert_eq!(histogram.summary().maximum(), 3.0);

    let Plot::Scatter(scatter) = table
        .plot(MeasureView::Scatter, [diameter, contrast], &gated)
        .expect("a scatter builds")
    else {
        panic!("a scatter mode builds a scatter");
    };
    let cells = |count: &dyn Fn(usize, usize) -> u64| {
        let mut sum = 0;
        for row in 0..96 {
            for column in 0..96 {
                sum += count(column, row);
            }
        }
        sum
    };
    assert_eq!(cells(&|x, y| scatter.total_at(x, y)), 4);
    assert_eq!(cells(&|x, y| scatter.gated_at(x, y)), 2);
    assert_eq!(scatter.peak(), 1);
    assert_eq!(scatter.x_summary().count(), 2);
    assert!((scatter.x_summary().mean() - 2.0).abs() < 1.0e-6);
    assert!((scatter.y_summary().mean() - 0.2).abs() < 1.0e-6);
    assert_eq!(scatter.correlation(), Some(1.0));
}

#[test]
fn widens_a_constant_column_into_a_readable_range() {
    let spots = spots([0.5, 0.5], [0.5, 0.5]);
    let channels = ["EGFP".to_string()];
    let table = Table::build(
        &[Frame::new(0, Objects::Spots(&spots))],
        None,
        &channels,
        1.0,
        Group::Object,
        grana_measure::Aggregate::Mean,
    );
    let diameter = column(&table, "diameter_um");
    let Plot::Histogram(histogram) = table
        .plot(MeasureView::Histogram, [diameter, diameter], &[0, 1])
        .expect("a histogram builds")
    else {
        panic!("a histogram mode builds a histogram");
    };
    assert!(histogram.low() < histogram.high());
    assert_eq!(
        (0..histogram.bins())
            .map(|bin| histogram.gated(bin))
            .sum::<u64>(),
        2
    );
}

fn column(table: &Table, key: &str) -> usize {
    table
        .columns()
        .iter()
        .position(|column| column.key() == key)
        .expect("the column exists")
}

fn spots(radii: [f32; 2], contrasts: [f32; 2]) -> SpotSet {
    let statistics = |contrast: f32| {
        SampleStatistics::new(
            contrast * 100.0,
            contrast * 90.0,
            contrast * 110.0,
            1.0,
            1.0,
        )
    };
    SpotSet::new(
        0,
        vec![
            Spot::new([1.0, 2.0, 3.0], radii[0], contrasts[0]),
            Spot::new([4.0, 5.0, 6.0], radii[1], contrasts[1]),
        ],
        grana_object::IntensityTable::new(
            1,
            vec![statistics(contrasts[0]), statistics(contrasts[1])],
        ),
    )
}
