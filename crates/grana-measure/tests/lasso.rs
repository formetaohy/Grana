use grana_measure::{Aggregate, Frame, Group, Lasso, Objects, Table};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};

#[test]
fn gates_rows_within_a_lasso_polygon_and_a_frame() {
    let first = spots();
    let second = spots();
    let channels = ["EGFP".to_string()];
    let table = Table::build(
        &[
            Frame::new(0, Objects::Spots(&first), &[]),
            Frame::new(1, Objects::Spots(&second), &[]),
        ],
        None,
        &channels,
        1.0,
        Group::Object,
        Aggregate::Mean,
    );
    let x = column(&table, "x_um");
    let y = column(&table, "y_um");

    let triangle = Lasso::new(x, y, vec![[0.5, 1.5], [2.5, 1.5], [1.5, 3.0]])
        .expect("a triangle gates a polygon");
    assert!(triangle.contains(1.0, 2.0));
    assert!(!triangle.contains(4.0, 5.0));
    assert_eq!(
        table.matching(&[], Some(&triangle), None),
        vec![0, 2],
        "the lasso keeps the rows inside its polygon"
    );

    let square = Lasso::new(x, y, vec![[1.0, 2.0], [4.0, 2.0], [4.0, 5.0], [1.0, 5.0]])
        .expect("a square gates a polygon");
    assert!(square.contains(1.0, 2.0), "a vertex lies inside the lasso");
    assert!(square.contains(1.0, 3.5), "an edge lies inside the lasso");
    assert!(
        square.contains(2.5, 3.5),
        "the interior lies inside the lasso"
    );
    assert!(!square.contains(5.0, 5.0), "the outside lies outside");
    assert_eq!(table.matching(&[], Some(&square), None), vec![0, 1, 2, 3]);

    let away = Lasso::new(x, y, vec![[10.0, 10.0], [11.0, 10.0], [10.0, 11.0]])
        .expect("an empty lasso is still a polygon");
    assert!(table.matching(&[], Some(&away), None).is_empty());

    assert_eq!(table.matching(&[], None, Some(1)), vec![2, 3]);
    assert_eq!(table.matching(&[], Some(&triangle), Some(0)), vec![0]);
    assert_eq!(table.count(None), 4);
    assert_eq!(table.count(Some(1)), 2);
    assert_eq!(table.count(Some(2)), 0);
}

#[test]
fn locates_the_nearest_observation_of_a_gated_row_set() {
    let first = spots();
    let second = spots();
    let channels = ["EGFP".to_string()];
    let table = Table::build(
        &[
            Frame::new(0, Objects::Spots(&first), &[]),
            Frame::new(1, Objects::Spots(&second), &[]),
        ],
        None,
        &channels,
        1.0,
        Group::Object,
        Aggregate::Mean,
    );
    let x = column(&table, "x_um");
    let y = column(&table, "y_um");
    let rows = (0..table.rows()).collect::<Vec<_>>();

    assert_eq!(
        table.nearest(&rows, &[x, y], &[1.1, 2.2], &[200.0, 100.0]),
        Some(0)
    );
    assert_eq!(
        table.nearest(&rows, &[x, y], &[4.0, 5.0], &[200.0, 100.0]),
        Some(1)
    );
    assert_eq!(
        table.nearest(&rows, &[x], &[3.6], &[100.0]),
        Some(1),
        "a one axis probe picks the closest value"
    );
    assert_eq!(
        table.nearest(&rows[2..], &[x], &[1.0], &[100.0]),
        Some(2),
        "a probe searches the gated rows only"
    );
    assert_eq!(table.nearest(&[], &[x], &[1.0], &[100.0]), None);
}

#[test]
fn rejects_degenerate_lassos() {
    assert!(Lasso::new(0, 1, vec![[0.0, 0.0], [1.0, 1.0]]).is_none());
    assert!(Lasso::new(0, 0, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]).is_none());
    assert!(Lasso::new(0, 1, vec![[0.0, 0.0], [f32::NAN, 1.0], [1.0, 0.0]]).is_none());
}

fn column(table: &Table, key: &str) -> usize {
    table
        .columns()
        .iter()
        .position(|column| column.key() == key)
        .expect("the column exists")
}

fn spots() -> SpotSet {
    let statistics = |value: f32| SampleStatistics::new(value, value, value, 0.0, value as f64);
    SpotSet::new(
        0,
        vec![
            Spot::new([1.0, 2.0, 0.0], 0.5, 0.25),
            Spot::new([4.0, 5.0, 0.0], 1.0, 0.5),
        ],
        IntensityTable::new(1, vec![statistics(10.0), statistics(20.0)]),
    )
}
