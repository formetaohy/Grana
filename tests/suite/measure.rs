use grana_measure::{Aggregate, Frame, Group, Objects, Table};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet, Track, TrackNode, TrackSet};

const CHANNEL: &str = "EGFP 488";

#[test]
fn summarises_spots_and_exports_a_csv() {
    let first = spots();
    let second = spots();
    let tracks = tracks();
    let channels = [CHANNEL.to_string()];
    let frames = [
        Frame::new(0, Objects::Spots(&first), &[], &[]),
        Frame::new(1, Objects::Spots(&second), &[], &[]),
    ];
    let table = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Object,
        Aggregate::Mean,
    );
    assert_eq!(table.rows(), 4);
    assert_eq!(value(&table, 0, "frame"), 1.0);
    assert_eq!(value(&table, 0, "time_s"), 0.0);
    assert_eq!(value(&table, 0, "x_um"), 1.0);
    assert_eq!(value(&table, 0, "track"), 1.0);
    assert_eq!(value(&table, 0, "egfp_488_intensity_mean"), 10.0);
    assert_eq!(value(&table, 0, "egfp_488_intensity_sum"), 100.0);
    assert_eq!(value(&table, 2, "frame"), 2.0);
    assert_eq!(value(&table, 2, "time_s"), 2.0);
    assert_eq!(table.observation(3).object(), Some(1));
    assert_eq!(table.observation(3).track(), Some(1));

    let per_track = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Track,
        Aggregate::Mean,
    );
    assert_eq!(per_track.rows(), 2);
    assert_eq!(value(&per_track, 0, "nodes"), 2.0);
    assert_eq!(value(&per_track, 0, "duration_s"), 1.0);
    assert_eq!(value(&per_track, 0, "straightness"), 1.0);
    assert_eq!(value(&per_track, 0, "egfp_488_intensity_mean_mean"), 10.0);

    let per_frame = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Timepoint,
        Aggregate::Sum,
    );
    assert_eq!(per_frame.rows(), 2);
    assert_eq!(value(&per_frame, 0, "objects"), 2.0);
    assert_eq!(value(&per_frame, 0, "diameter_um_sum"), 2.0);

    let csv = per_frame.csv(&[0, 1]);
    let lines = csv.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("frame,time_s,objects"));
}

fn value(table: &Table, row: usize, key: &str) -> f32 {
    let column = table
        .columns()
        .iter()
        .position(|column| column.key() == key)
        .unwrap_or_else(|| panic!("the {key} column exists"));
    table.value(row, column)
}

fn spots() -> SpotSet {
    let intensities = IntensityTable::new(
        1,
        vec![
            SampleStatistics::new(10.0, 9.0, 11.0, 0.5, 100.0),
            SampleStatistics::new(20.0, 19.0, 21.0, 0.5, 200.0),
        ],
    );
    SpotSet::new(
        0,
        vec![
            Spot::new([1.0, 2.0, 3.0], 0.5, 0.25),
            Spot::new([4.0, 5.0, 6.0], 0.5, 0.75),
        ],
        intensities,
    )
}

fn tracks() -> TrackSet {
    let first = Track::new(
        None,
        vec![
            TrackNode::new(0, 0, [1.0, 2.0, 3.0]),
            TrackNode::new(1, 0, [1.5, 2.0, 3.0]),
        ],
    );
    let second = Track::new(
        None,
        vec![
            TrackNode::new(0, 1, [4.0, 5.0, 6.0]),
            TrackNode::new(1, 1, [4.0, 5.0, 6.5]),
        ],
    );
    TrackSet::new(1.0, vec![first, second])
}
