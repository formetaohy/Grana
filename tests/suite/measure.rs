use std::sync::Arc;

use glam::Vec3;
use grana_measure::{Aggregate, Group, Kind, Mask, MeasureView, Plot, Table, Window};
use grana_object::{
    IntensityTable, SampleAccumulator, SampleStatistics, Spot, SpotSet, Track, TrackNode, TrackSet,
};
use grana_scene::{Dataset, Installed, Measure, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Extent, Region, Spacing};

use crate::support::{id, measure_source, node, spheres, spot_set, track};

const CHANNEL: &str = "EGFP 488";

#[test]
fn measures_every_object_in_micrometres() {
    let frames = measure_source(Kind::Spots, &[CHANNEL], [spots()]);
    let table = Table::build(
        Arc::clone(&frames),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the object table builds");
    let keys = table
        .columns()
        .iter()
        .map(|column| column.key())
        .collect::<Vec<_>>();
    assert!(keys.contains(&"x_um"), "a position is a physical length");
    assert!(
        keys.contains(&"diameter_um"),
        "a length is a physical length"
    );
    assert!(
        !keys.iter().any(|key| key.ends_with("_px")),
        "no column claims a pixel"
    );
    assert!(
        table
            .columns()
            .iter()
            .filter(|column| column.unit().spatial())
            .all(|column| column.unit() == grana_measure::Unit::Micrometer
                || column.unit() == grana_measure::Unit::SquareMicrometer
                || column.unit() == grana_measure::Unit::CubicMicrometer),
        "every spatial column carries a physical unit"
    );
}

#[test]
fn summarises_spots_and_exports_a_csv() {
    let first = spots();
    let second = spots();
    let tracks = Arc::new(tracks());
    let frames = measure_source(Kind::Spots, &[CHANNEL], [first, second]);
    let table = Table::build(
        Arc::clone(&frames),
        Some(Arc::clone(&tracks)),
        2.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the object table builds");
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
    assert_eq!(table.observation(3).object(), Some(id(1)));
    assert_eq!(table.observation(3).track(), Some(track(1)));

    let per_track = Table::build(
        Arc::clone(&frames),
        Some(Arc::clone(&tracks)),
        2.0,
        Group::Track,
        Aggregate::MEAN,
        None,
    )
    .expect("the track table builds");
    assert_eq!(per_track.rows(), 2);
    assert_eq!(value(&per_track, 0, "nodes"), 2.0);
    assert_eq!(value(&per_track, 0, "duration_s"), 1.0);
    assert_eq!(value(&per_track, 0, "straightness"), 1.0);
    assert_eq!(value(&per_track, 0, "egfp_488_intensity_mean_mean"), 10.0);
    let neighbour = per_track
        .column("nearest_distance_um_mean")
        .expect("an aggregated table keeps the aggregated neighbour distance");
    assert!(
        grana_scene::default_retained(&per_track).contains(&neighbour),
        "an aggregated neighbour distance stays visible by default"
    );

    let per_frame = Table::build(
        Arc::clone(&frames),
        Some(Arc::clone(&tracks)),
        2.0,
        Group::Timepoint,
        Aggregate::SUM,
        None,
    )
    .expect("the timepoint table builds");
    assert_eq!(per_frame.rows(), 2);
    assert_eq!(value(&per_frame, 0, "objects"), 2.0);
    assert_eq!(value(&per_frame, 0, "diameter_um_sum"), 2.0);
    let aggregated = per_frame
        .column("diameter_um_sum")
        .expect("the aggregate names its measurement");
    assert_eq!(
        per_frame.value(0, aggregated).support(),
        2,
        "an aggregate reports the samples it rests on rather than a bare number"
    );
    assert!(
        !per_frame.columns()[aggregated].complete(),
        "a measured column declares that it may be absent"
    );
    assert!(
        per_frame
            .column("frame")
            .is_some_and(|column| per_frame.columns()[column].complete()),
        "an identity column never goes absent"
    );
    let basis = Arc::new(
        Table::build(
            Arc::clone(&frames),
            Some(Arc::clone(&tracks)),
            2.0,
            Group::Object,
            Aggregate::MEAN,
            None,
        )
        .expect("the object table builds"),
    );
    let reused = Table::build(
        Arc::clone(&frames),
        Some(Arc::clone(&tracks)),
        2.0,
        Group::Timepoint,
        Aggregate::SUM,
        Some(Arc::clone(&basis)),
    )
    .expect("the timepoint table reuses its basis");
    assert_eq!(reused.rows(), per_frame.rows());
    for row in 0..reused.rows() {
        for column in 0..reused.columns().len() {
            assert_eq!(
                reused.value(row, column),
                per_frame.value(row, column),
                "the reused basis keeps the cell ({row}, {column})"
            );
        }
    }

    let csv = csv_of(&per_frame, &[0, 1]);
    let lines = csv.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("frame,time_s,objects"));

    let first_group = grouped(3.0);
    let second_group = grouped(5.0);
    let frame_of = measure_source(Kind::Spots, &[CHANNEL], [first_group, second_group]);
    let molecules = Table::build(
        Arc::clone(&frame_of),
        None,
        2.0,
        Group::Molecule,
        Aggregate::MEAN,
        None,
    )
    .expect("the molecule table builds");
    assert_eq!(
        molecules.rows(),
        2,
        "every localization group takes one row"
    );
    assert_eq!(value(&molecules, 0, "molecule"), 1.0);
    assert_eq!(value(&molecules, 0, "localizations"), 2.0);
    assert_eq!(value(&molecules, 0, "first_frame"), 1.0);
    assert_eq!(value(&molecules, 0, "last_frame"), 2.0);
    assert_eq!(value(&molecules, 0, "z_um"), 4.0);
    assert!((value(&molecules, 0, "spread_z_um") - 2.0f32.sqrt()).abs() < 1.0e-5);
    assert_eq!(value(&molecules, 0, "contrast_mean"), 0.5);
    assert_eq!(molecules.observation(0).molecule(), Some(1));
    assert!((value(&molecules, 0, "nearest_distance_um") - 27.0f32.sqrt()).abs() < 1.0e-5);
    assert!(
        csv_of(&molecules, &[0]).starts_with(
            "molecule,localizations,first_frame,last_frame,x_um,y_um,z_um,spread_x_um"
        )
    );
}

#[test]
fn materialises_measurement_columns_on_demand() {
    let frames = measure_source(Kind::Spots, &[CHANNEL], [spots(), spots()]);
    let table = Table::build(
        Arc::clone(&frames),
        None,
        2.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the object table builds");
    assert!(
        (0..table.columns().len()).all(|column| !table.cached(column)),
        "a built table keeps no column resident"
    );
    let diameter = table
        .column("diameter_um")
        .expect("the spots measure their diameter");
    let frame = table.column("frame").expect("the table numbers its frames");
    assert_eq!(table.value(0, diameter).present(), Some(1.0));
    assert_eq!(table.value(0, frame).present(), Some(1.0));
    assert!(table.cached(diameter) && table.cached(frame));
    table.retain([frame]);
    assert!(table.cached(frame), "a retained column stays resident");
    assert!(
        !table.cached(diameter),
        "a released column leaves the table"
    );
    assert_eq!(table.value(0, diameter).present(), Some(1.0));
    assert!(table.cached(diameter));
    let radii = [3.0f32, 1.0, 1.0];
    let anisotropic = Spot::gaussian(
        [0.5, 0.5, 0.5],
        radii.map(|radius| radius / grana_object::RADIUS_PER_SIGMA),
        1.0,
    );
    let equivalent = 2.0 * (radii[0] * radii[1] * radii[2]).cbrt();
    assert!(
        (2.0 * anisotropic.equivalent_radius() - equivalent).abs() < 1.0e-5,
        "an anisotropic spot measures its equivalent diameter, not its longest semi axis"
    );
    let level = grana_volume::Level::new(
        grana_volume::Extent::new(8, 8, 8),
        grana_volume::Spacing::new(1.0, 1.0, 1.0),
    );
    let frame = grana_object::Shape::spots(Arc::from([anisotropic]));
    let mask = grana_object::rasterize(&frame, level, 0.0).expect("the spot masks");
    assert!(
        mask.contains_cell([6, 4, 4]),
        "the mask reaches two voxels along the long semi axis"
    );
    assert!(
        !mask.contains_cell([4, 6, 4]),
        "the mask ends before two voxels along a short semi axis"
    );
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
    let measured = |aggregate: Aggregate, values: &[f32]| {
        aggregate
            .apply(values)
            .present()
            .expect("the aggregate measures these values")
    };
    assert_eq!(measured(median, &[1.0, 2.0, 3.0, 4.0]), 2.5);
    assert_eq!(measured(Aggregate::MAD, &[1.0, 2.0, 3.0, 4.0]), 1.0);
    assert_eq!(
        Aggregate::MAD.apply(&[f32::NAN]),
        grana_measure::Measured::Absent,
        "an aggregate over absent values reports no support rather than a NaN"
    );
    let quartile = Aggregate::quantile(25).expect("the lower quartile is supported");
    assert_eq!(measured(quartile, &[1.0, 2.0, 3.0, 4.0]), 1.75);
    assert_eq!(Aggregate::parse("median"), Some(median));
    assert_eq!(Aggregate::parse("p90"), Aggregate::quantile(90));
    assert_eq!(Aggregate::from_index(quartile.index()), Some(quartile));
}

#[test]
fn reports_an_unmeasured_object_as_absent() {
    assert_eq!(
        SampleAccumulator::new().statistics(),
        None,
        "an accumulator that holds no sample reports nothing rather than zero"
    );
    let frames = measure_source(Kind::Spots, &[CHANNEL], [unmeasured()]);
    let table = Table::build(
        Arc::clone(&frames),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the object table builds");
    let column = table
        .column("egfp_488_intensity_mean")
        .expect("the intensity column exists");
    assert_eq!(
        table.value(0, column),
        grana_measure::Measured::Absent,
        "an object whose voxels were never measured reports no intensity"
    );
    let text = csv_of(&table, &[0]);
    let mut lines = text.lines();
    let header = lines.next().expect("the statistics carry a header");
    let row = lines.next().expect("the statistics carry a row");
    let slot = header
        .split(',')
        .position(|key| key == "egfp_488_intensity_mean")
        .expect("the header names the intensity column");
    assert_eq!(
        row.split(',').nth(slot),
        Some(""),
        "an absent intensity leaves the csv field empty instead of writing zero"
    );
}

fn value(table: &Table, row: usize, key: &str) -> f32 {
    let column = table
        .columns()
        .iter()
        .position(|column| column.key() == key)
        .unwrap_or_else(|| panic!("the {key} column exists"));
    table.value(row, column).present().unwrap_or(f32::NAN)
}

fn csv_of(table: &Table, rows: &[u32]) -> String {
    let mut text = Vec::new();
    table
        .write_csv_rows(&mut text, rows, &mut |_, _| {})
        .expect("the statistics write as CSV");
    String::from_utf8(text).expect("the statistics are text")
}

fn spots() -> SpotSet {
    let intensities = IntensityTable::new(
        1,
        vec![
            Some(SampleStatistics::new(10.0, 9.0, 11.0, 0.5, 100.0)),
            Some(SampleStatistics::new(20.0, 19.0, 21.0, 0.5, 200.0)),
        ],
    );
    spot_set(
        0,
        vec![
            Spot::new([1.0, 2.0, 3.0], 0.5, 0.25),
            Spot::new([4.0, 5.0, 6.0], 0.5, 0.75),
        ],
        intensities,
    )
}

fn unmeasured() -> SpotSet {
    spot_set(
        0,
        vec![Spot::new([1.0, 2.0, 3.0], 0.5, 0.25)],
        IntensityTable::new(1, vec![None]),
    )
}

fn grouped(z: f32) -> SpotSet {
    let intensities = IntensityTable::new(
        1,
        vec![
            Some(SampleStatistics::new(10.0, 9.0, 11.0, 0.5, 100.0)),
            Some(SampleStatistics::new(20.0, 19.0, 21.0, 0.5, 200.0)),
        ],
    );
    spot_set(
        0,
        vec![
            Spot::grouped([1.0, 2.0, z], [0.5; 3], 0.5, Some(1)),
            Spot::grouped([4.0, 5.0, z + 3.0], [0.5; 3], 0.5, Some(2)),
        ],
        intensities,
    )
}

fn tracks() -> TrackSet {
    let first = Track::new(
        track(0),
        vec![
            TrackNode::new(0, id(0), [1.0, 2.0, 3.0]),
            TrackNode::new(1, id(0), [1.5, 2.0, 3.0]),
        ],
    );
    let second = Track::new(
        track(1),
        vec![
            TrackNode::new(0, id(1), [4.0, 5.0, 6.0]),
            TrackNode::new(1, id(1), [4.0, 5.0, 6.5]),
        ],
    );
    TrackSet::new(1.0, vec![first, second])
}

#[test]
fn edits_a_lineage_between_divisions_and_parents() {
    let mother = Track::new(
        track(0),
        vec![
            TrackNode::new(0, id(0), [0.0, 0.0, 0.0]),
            TrackNode::new(1, id(0), [1.0, 0.0, 0.0]),
        ],
    );
    let left = Track::new(
        track(1),
        vec![
            TrackNode::new(2, id(1), [2.0, 0.0, 0.0]),
            TrackNode::new(3, id(1), [3.0, 0.0, 0.0]),
        ],
    );
    let right = Track::new(
        track(2),
        vec![
            TrackNode::new(2, id(2), [2.0, 1.0, 0.0]),
            TrackNode::new(3, id(2), [3.0, 1.0, 0.0]),
        ],
    );
    let later = Track::new(
        track(3),
        vec![
            TrackNode::new(4, id(3), [4.0, 0.0, 0.0]),
            TrackNode::new(5, id(3), [5.0, 0.0, 0.0]),
        ],
    );
    let set = TrackSet::new(1.0, vec![mother, left, right, later]);
    let identities = |set: &TrackSet| {
        set.tracks()
            .iter()
            .map(|track| track.id())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        identities(&set),
        vec![track(0), track(1), track(2), track(3)]
    );
    assert!(set.can_divide(node(1, 0), node(2, 1), node(2, 2)));
    let divided = set
        .divide(node(1, 0), node(2, 1), node(2, 2))
        .expect("a division joins two daughter tracks");
    assert_eq!(divided.divisions(), 1);
    assert_eq!(
        divided.children(track(0)).collect::<Vec<_>>(),
        vec![track(1), track(2)]
    );
    assert_eq!(divided.parent(track(1)), Some(track(0)));
    assert_eq!(identities(&divided), identities(&set));

    let adopted = divided
        .reparent(node(4, 3), node(3, 1))
        .expect("a root track adopts an earlier end");
    assert_eq!(adopted.parent(track(3)), Some(track(1)));
    assert_eq!(adopted.divisions(), 1);
    assert_eq!(
        adopted.children(track(1)).collect::<Vec<_>>(),
        vec![track(3)]
    );

    let detached = adopted
        .detach(node(4, 3))
        .expect("an adopted track returns to a lineage start");
    assert_eq!(detached.parent(track(3)), None);
    assert_eq!(detached.divisions(), 1);

    let opened = divided
        .detach(node(2, 2))
        .expect("a division opens into separate lineages");
    assert_eq!(opened.divisions(), 0);
    assert_eq!(opened.parent(track(2)), None);
    assert_eq!(opened.parent(track(1)), Some(track(0)));

    let cut = set.cut(0, id(0)).expect("a track cuts between its nodes");
    assert_eq!(cut.track_at(0, id(0)), Some(track(0)));
    let tail = cut
        .track_at(1, id(0))
        .expect("the second half of the cut track is tracked");
    assert_ne!(tail, track(0), "the cut tail receives a new identity");
    assert_eq!(
        cut.track(tail).expect("the cut tail exists").nodes().len(),
        1
    );
    assert_eq!(cut.len(), set.len() + 1);
    assert_eq!(
        cut.track_at(2, id(1)),
        Some(track(1)),
        "a cut leaves the other tracks untouched"
    );
    let joined = cut
        .join(node(0, 0), node(1, 0))
        .expect("a cut track joins back together");
    assert_eq!(joined.len(), set.len());
    assert_eq!(joined.track_at(0, id(0)), Some(track(0)));
    assert_eq!(joined.track_at(1, id(0)), Some(track(0)));
    assert!(
        !joined.contains(tail),
        "the absorbed half gives up its identity"
    );

    let pruned = divided
        .without(2, id(1))
        .expect("a track node deletes from its set");
    assert!(
        !pruned.contains(track(1)),
        "the truncated track gives up its identity"
    );
    let remainder = pruned
        .track_at(3, id(1))
        .expect("the rest of the truncated track stays");
    assert_ne!(remainder, track(1));
    assert_eq!(pruned.track_at(2, id(2)), Some(track(2)));
    assert_eq!(pruned.divisions(), 0);
}

#[test]
fn restricts_the_measured_rows_to_the_analysis_box() {
    let volume = spheres(Extent::new(8, 8, 4), Spacing::new(1.0, 1.0, 1.0), &[], 0);
    let mut scene = Scene::new(Dataset::synthetic("box", volume.pyramid()));
    let spots = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        spots,
        0,
        ObjectFrame::from_spots(spot_set(
            0,
            vec![
                Spot::new([0.0, 0.0, 0.0], 1.0, 0.5),
                Spot::new([4.0, 0.0, 0.0], 1.0, 0.5),
            ],
            IntensityTable::new(
                1,
                vec![Some(SampleStatistics::new(500.0, 500.0, 500.0, 0.0, 500.0)); 2],
            ),
        )),
    );
    let mut measure = Measure::new();
    measure.select(spots, &scene);
    let request = measure
        .request(&scene, false)
        .expect("the table builds off the frame");
    let key = request.key();
    let table = Arc::new(request.build().expect("the spot table builds"));
    measure.install(key, Some(table));
    assert_eq!(measure.current_rows().expect("every row builds").total(), 2);
    measure.set_roi(Some(Region::new(
        Vec3::ZERO,
        [Vec3::X, Vec3::Y, Vec3::Z],
        Vec3::splat(1.0),
    )));
    let rows = measure.current_rows().expect("the boxed rows build");
    assert_eq!(
        rows.rows().len(),
        1,
        "the analysis box admits the one object inside it"
    );
    assert_eq!(rows.total(), 1);
}

#[test]
fn measures_cross_population_spatial_statistics() {
    let source = Spot::new([0.45, 0.5, 0.5], 0.02, 0.5);
    let target = Spot::new([0.55, 0.5, 0.5], 0.02, 0.5);
    let source = Table::build(
        measure_source(Kind::Spots, &[CHANNEL], [lone_spot(source)]),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the source table builds");
    let target = Table::build(
        measure_source(Kind::Spots, &[CHANNEL], [lone_spot(target)]),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the target table builds");
    let window = Window::new([0.0; 3], [1.0; 3]).expect("the unit cube is a window");
    let mask = Mask::all(1);
    assert_eq!(
        source.plot(MeasureView::Ripley, [0, 0], &mask, window, None),
        None,
        "one object holds no curve inside its own set"
    );
    let plot = source
        .plot(MeasureView::Ripley, [0, 0], &mask, window, Some(&target))
        .expect("a paired Ripley curve");
    let Plot::Ripley(ripley) = plot else {
        panic!("the paired view plots a Ripley curve");
    };
    assert_eq!(ripley.census().partners(), 1);
    for point in ripley.points() {
        let expected = if point.radius() >= 0.1 {
            0.620_350_5 - point.radius()
        } else {
            -point.radius()
        };
        assert!(
            (point.value() - expected).abs() < 1.0e-5,
            "L(r) at {} reads {} instead of {expected}",
            point.radius(),
            point.value()
        );
    }
    let plot = source
        .plot(
            MeasureView::NearestNeighbour,
            [0, 0],
            &mask,
            window,
            Some(&target),
        )
        .expect("a paired nearest curve");
    let Plot::NearestNeighbour(nearest) = plot else {
        panic!("the paired view plots a nearest curve");
    };
    assert_eq!(nearest.census().partners(), 1);
    for point in nearest.points() {
        let expected = if point.radius() >= 0.1 { 1.0 } else { 0.0 };
        assert!(
            (point.value() - expected).abs() < 1.0e-6,
            "G(r) at {} reads {} instead of {expected}",
            point.radius(),
            point.value()
        );
    }
    let both = Table::build(
        measure_source(
            Kind::Spots,
            &[CHANNEL],
            [spot_set(
                0,
                vec![
                    Spot::new([0.45, 0.5, 0.5], 0.02, 0.5),
                    Spot::new([0.55, 0.5, 0.5], 0.02, 0.5),
                ],
                IntensityTable::new(1, vec![None; 2]),
            )],
        ),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the single table builds");
    let mask = Mask::all(2);
    let Plot::Ripley(single) = both
        .plot(MeasureView::Ripley, [0, 0], &mask, window, None)
        .expect("a single-population Ripley curve")
    else {
        panic!("the single view plots a Ripley curve");
    };
    assert_eq!(single.census().partners(), 0);
    for (single, paired) in single.points().iter().zip(ripley.points()) {
        assert_eq!(
            single.value(),
            paired.value(),
            "the estimator reads the same for one set of two objects"
        );
    }
}

#[test]
fn pairs_a_second_object_set_for_spatial_statistics() {
    let volume = spheres(Extent::new(8, 8, 4), Spacing::new(1.0, 1.0, 1.0), &[], 0);
    let mut scene = Scene::new(Dataset::synthetic("pair", volume.pyramid()));
    let first = scene.add_object(ObjectKind::Spots, 0);
    let second = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        first,
        0,
        ObjectFrame::from_spots(spot_set(
            0,
            vec![
                Spot::new([0.45, 0.5, 0.5], 0.02, 0.5),
                Spot::new([0.45, 0.6, 0.5], 0.02, 0.5),
            ],
            IntensityTable::new(1, vec![None; 2]),
        )),
    );
    scene.install(
        second,
        0,
        ObjectFrame::from_spots(lone_spot(Spot::new([0.55, 0.5, 0.5], 0.02, 0.5))),
    );
    let window = Window::new([0.0; 3], [1.0; 3]).expect("the unit cube is a window");
    let mut measure = Measure::new();
    measure.select(first, &scene);
    let request = measure.request(&scene, false).expect("the table builds");
    let key = request.key();
    measure.install(
        key,
        Some(Arc::new(request.build().expect("the table holds"))),
    );
    measure.set_view(MeasureView::Ripley);
    let rows = measure.rows_request().expect("the rows build").build();
    measure.install_rows(rows);
    let request = measure
        .plot_request(Some(window))
        .expect("the single curve builds");
    let key = request.key();
    let plot = request.build().expect("the single curve holds");
    measure.install_plot(key, Some(plot));
    let Some(Plot::Ripley(single)) = measure.plot() else {
        panic!("the single curve installs");
    };
    assert_eq!(single.census().partners(), 0);
    measure.set_target(Some(first), &scene);
    assert_eq!(
        measure.target(),
        None,
        "an object set never pairs with itself"
    );
    measure.set_target(Some(second), &scene);
    let request = measure
        .request(&scene, false)
        .expect("the paired table builds");
    let key = request.key();
    assert_eq!(
        measure.install(
            key,
            Some(Arc::new(request.build().expect("the paired table holds")))
        ),
        Installed::Target
    );
    let request = measure
        .plot_request(Some(window))
        .expect("the paired curve builds");
    let key = request.key();
    let plot = request.build().expect("the paired curve holds");
    measure.install_plot(key, Some(plot));
    let Some(Plot::Ripley(paired)) = measure.plot() else {
        panic!("the paired curve installs");
    };
    assert_eq!(paired.census().partners(), 1);
    assert_eq!(measure.state(&scene).target, Some(second.raw()));
}

fn lone_spot(spot: Spot) -> SpotSet {
    spot_set(0, vec![spot], IntensityTable::new(1, vec![None]))
}

#[test]
fn measures_multi_class_neighbourhood_enrichment() {
    let layout = [
        ([0.50, 0.50, 1.00], 0.0),
        ([1.30, 0.50, 1.00], 0.0),
        ([0.90, 0.90, 1.00], 0.0),
        ([0.50, 1.30, 1.00], 0.0),
        ([1.30, 1.30, 1.00], 0.0),
        ([0.90, 0.50, 1.00], 1.0),
        ([0.50, 0.90, 1.00], 1.0),
        ([1.30, 0.90, 1.00], 1.0),
        ([0.90, 1.30, 1.00], 1.0),
        ([1.45, 1.45, 1.48], 2.0),
        ([1.40, 1.45, 1.48], 2.0),
        ([1.45, 1.40, 1.48], 2.0),
        ([1.45, 1.45, 1.50], 2.0),
    ];
    let objects = spot_set(
        0,
        layout
            .iter()
            .map(|(position, class)| {
                Spot::new(*position, 0.02, if *class == 2.0 { 0.8 } else { 0.2 })
            })
            .collect(),
        IntensityTable::new(1, vec![None; layout.len()]),
    );
    let source = measure_source(Kind::Spots, &[CHANNEL], [objects.clone()]);
    let window = Window::new([0.0; 3], [2.0; 3]).expect("the wide cube is a window");
    let unclassified = Table::build(
        Arc::clone(&source),
        None,
        1.0,
        Group::Object,
        Aggregate::MEAN,
        None,
    )
    .expect("the unclassified table builds");
    assert!(
        unclassified
            .plot(
                MeasureView::Enrichment,
                [0, 0],
                &Mask::all(unclassified.rows()),
                window,
                None,
            )
            .is_none(),
        "a table without a class column holds no neighbourhood enrichment"
    );
    let table = Table::build(source, None, 1.0, Group::Object, Aggregate::MEAN, None)
        .expect("the classified table builds")
        .with_column(
            grana_measure::class_column(),
            layout
                .iter()
                .map(|(_, class)| grana_measure::Measured::new(*class, 1))
                .collect(),
        );
    let mask = Mask::all(table.rows());
    let plot = table
        .plot(MeasureView::Enrichment, [0, 0], &mask, window, None)
        .expect("the classified table holds neighbourhood enrichment");
    let Plot::Enrichment(enrichment) = plot else {
        panic!("the enrichment view plots a matrix");
    };
    let codes = enrichment
        .classes()
        .iter()
        .map(grana_measure::EnrichmentClass::code)
        .collect::<Vec<_>>();
    assert_eq!(codes, vec![0, 1, 2]);
    assert_eq!(enrichment.classes()[0].index_cells(), 5);
    assert_eq!(enrichment.classes()[1].index_cells(), 4);
    assert_eq!(enrichment.classes()[2].index_cells(), 4);
    assert_eq!(enrichment.radius(), 0.5);
    assert_eq!(enrichment.permutations(), 199);
    let census = enrichment.census();
    assert_eq!(census.frames(), 1);
    assert_eq!(census.valid(), 1);
    assert_eq!(census.single(), 0);
    assert_eq!(census.objects(), 13);
    assert_eq!(census.partners(), 36);
    assert_eq!(census.sampled(), 13);
    let observed = [[0.0, 2.4, 0.0], [3.0, 0.0, 0.0], [0.0, 0.0, 3.0]];
    let ccr = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    for index in 0..codes.len() {
        for neighbour in 0..codes.len() {
            let pair = enrichment.pair(index, neighbour);
            assert_eq!(pair.index(), index);
            assert_eq!(pair.neighbour(), neighbour);
            assert!(
                (pair.observed() - observed[index][neighbour]).abs() < 1.0e-12,
                "S({index}, {neighbour}) reads {} instead of {}",
                pair.observed(),
                observed[index][neighbour]
            );
            assert!(
                (pair.ccr() - ccr[index][neighbour]).abs() < 1.0e-12,
                "CCR({index}, {neighbour}) reads {} instead of {}",
                pair.ccr(),
                ccr[index][neighbour]
            );
            assert!(
                pair.q() < 0.05,
                "the q value of ({index}, {neighbour}) stays significant"
            );
            assert!(pair.q() >= pair.p());
            assert!(pair.significant());
        }
    }
    assert_eq!(enrichment.significant(), codes.len() * codes.len());
    for (index, neighbour) in [(0, 1), (1, 0), (2, 2)] {
        assert!(
            enrichment.pair(index, neighbour).z() > 2.0,
            "the enrichment of ({index}, {neighbour}) is positive"
        );
    }
    for (index, neighbour) in [(0, 0), (0, 2), (1, 1), (1, 2), (2, 0), (2, 1)] {
        assert!(
            enrichment.pair(index, neighbour).z() < -2.0,
            "the depletion of ({index}, {neighbour}) is negative"
        );
    }
    let text = enrichment.table();
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 10);
    assert_eq!(
        lines[0],
        "class_i,class_j,s_obs,mean,sd,z,q,ccr,significant"
    );
    assert!(lines[1].starts_with("0,0,0.0000,"));
    assert!(lines[2].starts_with("0,1,2.4000,"));
    assert!(lines[4].starts_with("1,0,3.0000,"));
    assert!(lines[9].starts_with("2,2,3.0000,"));
    assert!(lines[1..].iter().all(|line| line.ends_with(",1")));
    let volume = spheres(Extent::new(8, 8, 4), Spacing::new(1.0, 1.0, 1.0), &[], 0);
    let mut scene = Scene::new(Dataset::synthetic("enrichment", volume.pyramid()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(set, 0, ObjectFrame::from_spots(objects));
    scene
        .train_classifier(
            set,
            &[grana_measure::Measurement::Contrast],
            &(0..9).collect::<Vec<_>>(),
        )
        .expect("the contrast separates the two classes");
    let measured = scene
        .table(set, Group::Object, Aggregate::MEAN)
        .expect("the classified set measures its table");
    assert!(measured.column(grana_measure::CLASS_KEY).is_some());
    let Plot::Enrichment(scene_enrichment) = measured
        .plot(
            MeasureView::Enrichment,
            [0, 0],
            &Mask::all(measured.rows()),
            window,
            None,
        )
        .expect("the classifier writes a class column")
    else {
        panic!("the classified set plots an enrichment matrix");
    };
    assert_eq!(scene_enrichment.classes().len(), 2);
    let repeated = table
        .plot(MeasureView::Enrichment, [0, 0], &mask, window, None)
        .expect("the enrichment repeats");
    assert_eq!(Plot::Enrichment(enrichment), repeated);
}
