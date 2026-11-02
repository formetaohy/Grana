use grana_object::{Track, TrackNode, TrackSet};

fn node(time: u32, object: usize) -> TrackNode {
    TrackNode::new(time, object, [time as f32, 0.0, 0.0])
}

fn line(time: u32, object: usize, frames: u32) -> Track {
    Track::new(
        None,
        (time..time + frames)
            .map(|time| node(time, object))
            .collect(),
    )
}

#[test]
fn cuts_and_joins_tracks() {
    let set = TrackSet::new(1.0, vec![line(0, 0, 3), line(3, 1, 2)]);
    let cut = set
        .cut(1, 0)
        .expect("the track breaks before its last frame");
    assert_eq!(cut.len(), 3);
    assert_eq!(cut.track_at(1, 0), Some(0));
    assert_eq!(cut.track(0).expect("the head exists").nodes().len(), 2);
    assert!(cut.track(1).expect("the tail exists").parent().is_none());

    let rejoined = cut.join((1, 0), (2, 0)).expect("the head rejoins the tail");
    assert_eq!(rejoined.len(), 2);
    assert_eq!(
        rejoined.track(0).expect("the track exists").nodes().len(),
        3
    );

    let merged = set
        .join((2, 0), (3, 1))
        .expect("the track continues into the next track");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged.track(0).expect("the track exists").nodes().len(), 5);
    assert!(!set.can_join((2, 0), (0, 0)));
}

#[test]
fn removes_and_keeps_divisions() {
    let parent = Track::new(None, vec![node(0, 0)]);
    let first = Track::new(Some(0), vec![node(1, 1), node(2, 1)]);
    let second = Track::new(Some(0), vec![node(1, 2), node(2, 2)]);
    let set = TrackSet::new(1.0, vec![parent, first, second]);
    assert_eq!(set.divisions(), 1);

    let split = set.cut(0, 0).expect("the division breaks");
    assert_eq!(split.divisions(), 0);
    assert_eq!(split.len(), 3);

    let merged = split
        .join((0, 0), (1, 1))
        .expect("the parent rejoins a daughter");
    assert_eq!(merged.len(), 2);
    assert_eq!(merged.divisions(), 0);
    let track = merged.track_at(2, 1).expect("the daughter is tracked");
    assert_eq!(
        merged.track(track).expect("the track exists").nodes().len(),
        3
    );
}
