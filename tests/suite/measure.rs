use grana_measure::{
    Aggregate, DENSITY_POINTS, Frame, Group, Mask, MeasureView, Objects, Plot, Table, Window,
};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet, Track, TrackNode, TrackSet};
use grana_scene::{Curation, TrackAnchor, TrackEdit};

const CHANNEL: &str = "EGFP 488";

fn window() -> Window {
    Window::new([-10.0; 3], [10.0; 3]).expect("the test window spans the volume")
}

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
        Aggregate::MEAN,
    );
    assert_eq!(table.rows(), 4);
    assert_eq!(value(&table, 0, "frame"), 1.0);
    assert_eq!(value(&table, 0, "time_s"), 0.0);
    assert_eq!(value(&table, 0, "x_um"), 1.0);
    assert_eq!(value(&table, 0, "track"), 1.0);
    assert_eq!(value(&table, 0, "egfp_488_intensity_mean"), 10.0);
    assert_eq!(value(&table, 0, "egfp_488_intensity_sum"), 100.0);
    assert!((value(&table, 0, "nearest_distance_um") - 27.0f32.sqrt()).abs() < 1.0e-5);
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
        Aggregate::MEAN,
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
        Aggregate::SUM,
    );
    assert_eq!(per_frame.rows(), 2);
    assert_eq!(value(&per_frame, 0, "objects"), 2.0);
    assert_eq!(value(&per_frame, 0, "diameter_um_sum"), 2.0);

    let csv = per_frame.csv(&[0, 1]);
    let lines = csv.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("frame,time_s,objects"));
}

#[test]
fn trends_raw_frames_of_every_grouping() {
    let first = spots();
    let second = spots();
    let tracks = tracks();
    let channels = [CHANNEL.to_string()];
    let frames = [
        Frame::new(0, Objects::Spots(&first), &[], &[]),
        Frame::new(1, Objects::Spots(&second), &[], &[]),
    ];
    let per_frame = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Timepoint,
        Aggregate::MEAN,
    );
    let column = per_frame
        .column("egfp_488_intensity_mean_mean")
        .expect("the timepoints aggregate the raw intensity");
    let trend = per_frame.trend(column, &Mask::all(per_frame.rows()));
    assert_eq!(trend.frames(), 2);
    assert_eq!(trend.total(), 4);
    assert_eq!(trend.points().len(), 2);
    assert_eq!(trend.points()[0].value(), 15.0);
    assert_eq!(trend.points()[0].count(), 2);
    assert!(trend.points()[0].lower() < 15.0 && trend.points()[0].upper() > 15.0);

    let per_track = Table::build(
        &frames,
        Some(&tracks),
        &channels,
        2.0,
        Group::Track,
        Aggregate::MEAN,
    );
    let column = per_track
        .column("egfp_488_intensity_mean_mean")
        .expect("the tracks aggregate the raw intensity");
    let trend = per_track.trend(column, &Mask::all(per_track.rows()));
    assert_eq!(trend.frames(), 2);
    assert_eq!(trend.total(), 4);
    assert_eq!(trend.points().len(), 2);
    assert_eq!(trend.points()[1].value(), 15.0);

    let summary = per_track
        .column("nodes")
        .expect("the tracks summarise their nodes");
    let trend = per_track.trend(summary, &Mask::all(per_track.rows()));
    assert_eq!(trend.points().len(), 2);
    assert_eq!(trend.points()[0].value(), 2.0);
    assert_eq!(trend.points()[0].count(), 2);
}

#[test]
fn estimates_spatial_point_patterns() {
    let first = spots();
    let second = spots();
    let channels = [CHANNEL.to_string()];
    let frames = [
        Frame::new(0, Objects::Spots(&first), &[], &[]),
        Frame::new(1, Objects::Spots(&second), &[], &[]),
    ];
    let table = Table::build(
        &frames,
        None,
        &channels,
        2.0,
        Group::Object,
        Aggregate::MEAN,
    );
    let rows = Mask::all(table.rows());
    let plot = table
        .plot(MeasureView::Ripley, [0, 0], &rows, window())
        .expect("the points spread across a sampling window");
    let Plot::Ripley(ripley) = plot else {
        panic!("the spatial view plots a radial distribution");
    };
    assert_eq!(ripley.frames(), 2);
    assert_eq!(ripley.objects(), 4);
    assert_eq!(ripley.points().len(), 24);
    assert!(ripley.radius() > 0.0);
    assert!(ripley.low() <= 0.0 && ripley.high() >= 0.0);
    assert!(ripley.simulations() >= 9);
    let test = ripley
        .test()
        .expect("the CSR envelope tests the point pattern");
    assert!((0.0..=1.0).contains(&test.p()));
    assert_eq!(test.simulations(), ripley.simulations());
    assert_eq!(
        test.beyond(),
        ripley
            .points()
            .iter()
            .filter(|point| point.beyond())
            .count() as u32
    );
    assert!(
        ripley
            .points()
            .iter()
            .all(|point| point.envelope().is_none_or(|(low, high)| low <= high))
    );
    assert!(
        ripley
            .points()
            .iter()
            .filter(|point| point.envelope().is_some())
            .count()
            >= 8
    );

    let plane = (0..64)
        .map(|index| [(index % 8) as f32 * 3.0, (index / 8) as f32 * 3.0, 0.0])
        .collect::<Vec<_>>();
    let distances = grana_measure::neighbour_distances(&plane);
    assert!(
        distances
            .iter()
            .all(|distance| distance.is_finite() && *distance >= 3.0 - 1.0e-4)
    );
}

#[test]
fn groups_measurements_by_frame_for_distributions() {
    let first = spots();
    let second = spots();
    let channels = [CHANNEL.to_string()];
    let frames = [
        Frame::new(0, Objects::Spots(&first), &[], &[]),
        Frame::new(1, Objects::Spots(&second), &[], &[]),
    ];
    let table = Table::build(
        &frames,
        None,
        &channels,
        2.0,
        Group::Object,
        Aggregate::MEAN,
    );
    let column = table
        .column("egfp_488_intensity_mean")
        .expect("the spots carry their mean intensity");
    let rows = Mask::all(table.rows());
    let plot = table
        .plot(MeasureView::Distribution, [column, 0], &rows, window())
        .expect("a distribution plots its column");
    let Plot::Distribution(distribution) = plot else {
        panic!("the distribution view plots a distribution");
    };
    assert_eq!(distribution.column(), column);
    assert_eq!(distribution.total(), 4);
    assert_eq!(distribution.gated(), 4);
    assert_eq!(distribution.groups().len(), 2);
    assert_eq!(distribution.groups()[0].label(), "Frame 1");
    assert_eq!(distribution.groups()[1].label(), "Frame 2");
    let series = distribution.groups()[0].total();
    assert_eq!(series.count(), 2);
    assert_eq!(series.median(), 15.0);
    assert_eq!(series.lower_quartile(), 12.5);
    assert_eq!(series.upper_quartile(), 17.5);
    assert_eq!(series.whisker_low(), 10.0);
    assert_eq!(series.whisker_high(), 20.0);
    assert!(series.outliers().is_empty());
    assert_eq!(series.density().len(), DENSITY_POINTS);
    assert!(series.density().iter().copied().fold(0.0f32, f32::max) >= 0.999);

    let first_frame = Mask::of(table.rows(), |row| row < 2);
    let plot = table
        .plot(
            MeasureView::Distribution,
            [column, 0],
            &first_frame,
            window(),
        )
        .expect("a gated distribution plots its column");
    let Plot::Distribution(gated) = plot else {
        panic!("the distribution view plots a distribution");
    };
    assert_eq!(gated.total(), 4);
    assert_eq!(gated.gated(), 2);
    assert_eq!(
        gated.groups()[0]
            .gated()
            .expect("the first frame is gated in")
            .count(),
        2
    );
    assert!(gated.groups()[1].gated().is_none());
}

#[test]
fn tests_group_significance() {
    let a = [1.0, 2.0, 3.0];
    let b = [4.0, 5.0, 6.0];
    let welch = grana_measure::WelchTest::of(&a, &b).expect("two three-sample groups test");
    assert_eq!(welch.n_a(), 3);
    assert_eq!(welch.mean_a(), 2.0);
    assert_eq!(welch.mean_b(), 5.0);
    assert_eq!(welch.deviation_a(), 1.0);
    assert_eq!(welch.difference(), -3.0);
    assert!((welch.t() + 3.674_234_614_174_767_3).abs() < 1.0e-12);
    assert!((welch.df() - 4.0).abs() < 1.0e-12);
    assert!((welch.p() - 0.021_311_641_128_756_713).abs() < 1.0e-11);
    assert!(
        (grana_measure::hedges_g(&a, &b).expect("the pooled spread is positive") + 2.4).abs()
            < 1.0e-12
    );
    assert!(grana_measure::WelchTest::of(&a[..1], &b).is_none());
    assert!(
        (grana_measure::student_two_sided(2.0, 10.0) - 0.073_388_034_770_740_37).abs() < 1.0e-11
    );
    assert_eq!(grana_measure::student_two_sided(0.0, 5.0), 1.0);
    let corrected = grana_measure::benjamini_hochberg(&[0.001, 0.01, 0.04]);
    assert!((corrected[0] - 0.003).abs() < 1.0e-12);
    assert!((corrected[1] - 0.015).abs() < 1.0e-12);
    assert!((corrected[2] - 0.04).abs() < 1.0e-12);
    assert_eq!(grana_measure::stars(0.0009), "***");
    assert_eq!(grana_measure::stars(0.02), "*");
    assert_eq!(grana_measure::stars(0.2), "ns");
}

#[test]
fn tests_clustered_group_significance() {
    let images = |values: [[f64; 3]; 2]| {
        values.map(|image| {
            grana_measure::ClusterSample::of(&image).expect("an image spreads across objects")
        })
    };
    let a = images([[1.0, 2.0, 3.0], [2.0, 3.0, 4.0]]);
    let b = images([[10.0, 11.0, 12.0], [11.0, 12.0, 13.0]]);
    let test = grana_measure::ClusteredWelch::of(&a, &b).expect("two images per group test");
    assert_eq!(test.samples_a(), 6);
    assert_eq!(test.clusters_a(), 2);
    assert_eq!(test.mean_a(), 2.5);
    assert!((test.difference() + 9.0).abs() < 1.0e-12);
    assert!((test.error() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-12);
    assert!((test.t() + 12.727_922_061_357_855).abs() < 1.0e-9);
    assert!((test.df() - 2.0).abs() < 1.0e-12);
    assert!((test.p() - 0.006_116_3).abs() < 1.0e-6);
    assert!(
        (test.deviation_a().expect("six objects spread") - 1.048_808_848_170_151_6).abs() < 1.0e-12
    );
    assert!(
        grana_measure::ClusteredWelch::of(&a[..1], &b).is_none(),
        "a single image carries no cluster variance"
    );

    let singles = |values: [f64; 3]| {
        values.map(|value| grana_measure::ClusterSample::single(value).expect("a counted image"))
    };
    let welch = grana_measure::WelchTest::of(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0])
        .expect("two three-object groups test");
    let degenerated =
        grana_measure::ClusteredWelch::of(&singles([1.0, 2.0, 3.0]), &singles([4.0, 5.0, 6.0]))
            .expect("images of one object test");
    assert!((degenerated.t() - welch.t()).abs() < 1.0e-12);
    assert!((degenerated.df() - welch.df()).abs() < 1.0e-12);
    assert!((degenerated.p() - welch.p()).abs() < 1.0e-12);
    assert!(
        (degenerated
            .design_effect()
            .expect("the design effect reads")
            - 1.0)
            .abs()
            < 1.0e-12
    );
}

#[test]
fn summarises_robust_statistics() {
    let a = [1.0, 2.0, 3.0];
    let b = [4.0, 5.0, 6.0];
    let sample =
        grana_measure::Sample::of(&[1.0, 2.0, 3.0, 4.0, 5.0]).expect("a sample holds values");
    assert_eq!(sample.median(), 3.0);
    assert_eq!(sample.quantile(25.0), 2.0);
    assert_eq!(sample.quantile(75.0), 4.0);
    assert_eq!(sample.mad(), 1.0);
    assert!(
        (sample.deviation().expect("five values spread") - 1.581_138_830_084_189_8).abs() < 1.0e-12
    );
    let interval = sample
        .mean_interval(0.95)
        .expect("five values bound a mean");
    assert!((interval.0 - 1.036_756_838_522_439).abs() < 1.0e-9);
    assert!((interval.1 - 4.963_243_161_477_561).abs() < 1.0e-9);
    let ordered = grana_measure::Sample::of(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])
        .expect("ten values bound a median");
    let median = ordered
        .median_interval(0.95)
        .expect("ten values order a median");
    assert_eq!(median, (1.0, 8.0));
    assert!(grana_measure::Sample::of(&[f64::NAN]).is_none());

    let rank = grana_measure::MannWhitney::of(&a, &b).expect("two groups rank");
    assert_eq!(rank.n_a(), 3);
    assert_eq!(rank.n_b(), 3);
    assert_eq!(rank.u(), 0.0);
    assert_eq!(rank.delta(), -1.0);
    assert!((rank.p() - 0.080_855_596_178_832_73).abs() < 1.0e-6);
    assert_eq!(rank.median_a(), 2.0);
    assert_eq!(rank.median_b(), 5.0);

    let median = Aggregate::quantile(50).expect("the median is a supported quantile");
    assert_eq!(median.apply(&[1.0, 2.0, 3.0, 4.0]), 2.5);
    assert_eq!(Aggregate::MAD.apply(&[1.0, 2.0, 3.0, 4.0]), 1.0);
    let quartile = Aggregate::quantile(25).expect("the lower quartile is supported");
    assert_eq!(quartile.apply(&[1.0, 2.0, 3.0, 4.0]), 1.75);
    assert_eq!(Aggregate::parse("median"), Some(median));
    assert_eq!(Aggregate::parse("p90"), Aggregate::quantile(90));
    assert_eq!(Aggregate::from_index(quartile.index()), Some(quartile));
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

#[test]
fn edits_a_lineage_between_divisions_and_parents() {
    let mother = Track::new(
        None,
        vec![
            TrackNode::new(0, 0, [0.0, 0.0, 0.0]),
            TrackNode::new(1, 0, [1.0, 0.0, 0.0]),
        ],
    );
    let left = Track::new(
        None,
        vec![
            TrackNode::new(2, 1, [2.0, 0.0, 0.0]),
            TrackNode::new(3, 1, [3.0, 0.0, 0.0]),
        ],
    );
    let right = Track::new(
        None,
        vec![
            TrackNode::new(2, 2, [2.0, 1.0, 0.0]),
            TrackNode::new(3, 2, [3.0, 1.0, 0.0]),
        ],
    );
    let later = Track::new(
        None,
        vec![
            TrackNode::new(4, 3, [4.0, 0.0, 0.0]),
            TrackNode::new(5, 3, [5.0, 0.0, 0.0]),
        ],
    );
    let set = TrackSet::new(1.0, vec![mother, left, right, later]);
    assert!(set.can_divide((1, 0), (2, 1), (2, 2)));
    let divided = set
        .divide((1, 0), (2, 1), (2, 2))
        .expect("a division joins two daughter tracks");
    assert_eq!(divided.divisions(), 1);
    assert_eq!(divided.children(0).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(divided.track(1).and_then(Track::parent), Some(0));

    let adopted = divided
        .reparent((4, 3), (3, 1))
        .expect("a root track adopts an earlier end");
    assert_eq!(adopted.track(3).and_then(Track::parent), Some(1));
    assert_eq!(adopted.divisions(), 1);
    assert_eq!(adopted.children(1).collect::<Vec<_>>(), vec![3]);

    let detached = adopted
        .detach((4, 3))
        .expect("an adopted track returns to a lineage start");
    assert_eq!(detached.track(3).and_then(Track::parent), None);
    assert_eq!(detached.divisions(), 1);

    let opened = divided
        .detach((2, 2))
        .expect("a division opens into separate lineages");
    assert_eq!(opened.divisions(), 0);
    assert_eq!(opened.track(2).and_then(Track::parent), None);
    assert_eq!(opened.track(1).and_then(Track::parent), Some(0));
}

#[test]
fn restores_lineage_edits_that_cross_frames() {
    let at = |time: u32, object: usize| {
        TrackAnchor::new(time, object, [time as f32, object as f32, 0.0])
    };
    let removed = vec![Vec::new(); 8];
    let edits = vec![
        TrackEdit::Join {
            source: at(1, 0),
            target: at(3, 1),
        },
        TrackEdit::Divide {
            parent: at(2, 1),
            first: at(4, 2),
            second: at(4, 3),
        },
        TrackEdit::Reparent {
            child: at(5, 4),
            parent: at(3, 1),
        },
        TrackEdit::Detach { child: at(6, 5) },
    ];
    let curation = Curation::restored(8, &removed, &[], &edits)
        .expect("lineage edits name detections at their own frames");
    assert_eq!(curation.edit_count(), 4);
    assert_eq!(curation.edits(), edits.as_slice());
}
