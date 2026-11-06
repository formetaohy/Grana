use grana_measure::{Aggregate, Filter, Frame, Group, Objects, Table};
use grana_object::{SampleStatistics, Spot, SpotSet, Track, TrackNode, TrackSet};

const CHANNEL: &str = "EGFP 488";

#[test]
fn summarises_spots_by_object_track_and_timepoint() {
    let first = spots();
    let second = spots();
    let tracks = tracks();
    let channels = [CHANNEL.to_string()];
    let frames = [
        Frame::new(0, Objects::Spots(&first), &[], &[]),
        Frame::new(1, Objects::Spots(&second), &[], &[]),
    ];

    let objects = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Object,
        Aggregate::Mean,
    );
    assert_eq!(
        objects.columns().len(),
        4 + 3 + 2 + 5,
        "identity and measurement columns"
    );
    assert_eq!(objects.rows(), 4);
    assert_eq!(column(&objects, "egfp_488_intensity_mean"), 9);
    assert_eq!(row(&objects, 0, "frame"), 1.0);
    assert_eq!(row(&objects, 0, "time_s"), 0.0);
    assert_eq!(row(&objects, 0, "object"), 1.0);
    assert_eq!(row(&objects, 0, "track"), 1.0);
    assert_eq!(row(&objects, 0, "x_um"), 1.0);
    assert_eq!(row(&objects, 0, "diameter_um"), 1.0);
    assert_eq!(row(&objects, 0, "contrast"), 0.25);
    assert_eq!(row(&objects, 0, "egfp_488_intensity_mean"), 10.0);
    assert_eq!(row(&objects, 0, "egfp_488_intensity_sum"), 100.0);
    assert_eq!(row(&objects, 2, "frame"), 2.0);
    assert_eq!(row(&objects, 2, "time_s"), 2.0);
    assert_eq!(row(&objects, 2, "track"), 1.0);
    assert_eq!(objects.observation(3).object(), Some(1));
    assert_eq!(objects.observation(3).track(), Some(1));

    let per_track = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Track,
        Aggregate::Mean,
    );
    assert_eq!(per_track.rows(), 2);
    assert_eq!(column(&per_track, "duration_s"), 7);
    assert_eq!(column(&per_track, "diameter_um_mean"), 12);
    assert_eq!(row(&per_track, 0, "track"), 1.0);
    assert_eq!(row(&per_track, 0, "parent"), 0.0);
    assert_eq!(row(&per_track, 0, "daughters"), 0.0);
    assert_eq!(row(&per_track, 0, "first_frame"), 1.0);
    assert_eq!(row(&per_track, 0, "last_frame"), 2.0);
    assert_eq!(row(&per_track, 0, "nodes"), 2.0);
    assert_eq!(row(&per_track, 0, "frames"), 2.0);
    assert_eq!(row(&per_track, 0, "duration_s"), 1.0);
    assert_eq!(row(&per_track, 0, "length_um"), 0.5);
    assert_eq!(row(&per_track, 0, "displacement_um"), 0.5);
    assert_eq!(row(&per_track, 0, "speed_um_s"), 0.5);
    assert_eq!(row(&per_track, 0, "straightness"), 1.0);
    assert_eq!(row(&per_track, 0, "diameter_um_mean"), 1.0);
    assert_eq!(row(&per_track, 0, "egfp_488_intensity_mean_mean"), 10.0);
    assert_eq!(per_track.observation(0).track(), Some(0));

    let per_frame = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Timepoint,
        Aggregate::Sum,
    );
    assert_eq!(per_frame.rows(), 2);
    assert_eq!(row(&per_frame, 0, "objects"), 2.0);
    assert_eq!(row(&per_frame, 0, "diameter_um_sum"), 2.0);
    assert_eq!(row(&per_frame, 0, "egfp_488_intensity_mean_sum"), 30.0);
    assert_eq!(per_frame.observation(1).time(), Some(1));
}

#[test]
fn filters_sorts_and_exports_rows() {
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
        1.0,
        Group::Object,
        Aggregate::Mean,
    );
    let x = column(&table, "x_um");
    let matched = table.matching(&[Filter::new(x, 3.5, 6.0)], None, None);
    assert_eq!(matched, vec![1, 3]);

    let empty = table.matching(&[Filter::new(x, 10.0, 20.0)], None, None);
    assert!(empty.is_empty());

    let mut ordered = matched.clone();
    table.sort(&mut ordered, column(&table, "frame"), true);
    assert_eq!(ordered, vec![3, 1]);

    let csv = table.csv(&ordered);
    let lines = csv.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("frame,time_s,object,track,x_um,y_um,z_um"));
    assert!(lines[0].ends_with("egfp_488_intensity_std"));
    assert!(lines[1].starts_with("2,1.000,2,2,4.000"));
    assert_eq!(table.extent(x), Some((1.0, 4.0)));
}

fn column(table: &Table, key: &str) -> usize {
    table
        .columns()
        .iter()
        .position(|column| column.key() == key)
        .expect("the column exists")
}

fn row(table: &Table, row: usize, key: &str) -> f32 {
    table.value(row, column(table, key))
}

fn spots() -> SpotSet {
    let intensities = grana_object::IntensityTable::new(
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
