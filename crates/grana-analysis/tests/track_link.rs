use grana_analysis::TrackBuilder;
use grana_object::TrackParams;

#[test]
fn links_motion_and_closes_gaps() {
    let params = TrackParams::new(1.0, 1, false, 2.0);
    let mut builder = TrackBuilder::new(params);
    builder.push(0, &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]);
    builder.push(1, &[[0.5, 0.0, 0.0]]);
    builder.push(2, &[[1.0, 0.0, 0.0], [5.2, 0.0, 0.0]]);
    let set = builder.finish(1.0);

    assert_eq!(set.len(), 2);
    let moving = set.track_at(2, 0).expect("the moving object is tracked");
    let joined = set.track_at(2, 1).expect("the gap is closed");
    assert_ne!(moving, joined);
    assert_eq!(
        set.track(moving).expect("the track exists").nodes().len(),
        3
    );
    let joined = set.track(joined).expect("the track exists");
    assert_eq!(joined.nodes().len(), 2);
    assert_eq!(joined.nodes()[1].time(), 2);
    let statistics = set.statistics(moving).expect("the track exists");
    assert!((statistics.length() - 1.0).abs() < 1.0e-5);
    assert!((statistics.speed() - 0.5).abs() < 1.0e-5);
    assert!((statistics.straightness() - 1.0).abs() < 1.0e-5);
}

#[test]
fn splits_dividing_objects_into_lineages() {
    let params = TrackParams::new(0.8, 0, true, 2.0);
    let mut builder = TrackBuilder::new(params);
    builder.push(0, &[[0.0, 0.0, 0.0]]);
    builder.push(1, &[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]]);
    builder.push(2, &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    builder.push(3, &[[-1.4, 0.0, 0.0], [1.4, 0.0, 0.0]]);
    let set = builder.finish(2.0);

    assert_eq!(set.len(), 3);
    assert_eq!(set.divisions(), 1);
    let parent = set.track(0).expect("the parent exists");
    assert!(parent.parent().is_none());
    assert_eq!(parent.nodes().len(), 1);
    let children = set.children(0).collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    for child in children {
        let track = set.track(child).expect("the daughter exists");
        assert_eq!(track.parent(), Some(0));
        assert_eq!(track.nodes().len(), 3);
    }
}

#[test]
fn keeps_diverging_objects_apart_without_lineage() {
    let params = TrackParams::new(0.8, 0, false, 2.0);
    let mut builder = TrackBuilder::new(params);
    builder.push(0, &[[0.0, 0.0, 0.0]]);
    builder.push(1, &[[-0.6, 0.0, 0.0], [0.6, 0.0, 0.0]]);
    builder.push(2, &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let set = builder.finish(1.0);

    assert_eq!(set.len(), 2);
    assert_eq!(set.divisions(), 0);
    assert!(set.tracks().iter().all(|track| track.parent().is_none()));
}
