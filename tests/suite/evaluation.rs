use grana_evaluation::{Graph, Marker, Matching, TrackRow, Truth};
use grana_object::{ObjectId, Track, TrackId, TrackNode, TrackSet};

#[test]
fn matches_maximum_marker_pairs() {
    let truth = Truth::new(
        vec![vec![
            Marker::new(1, 0, [0.0, 0.0, 0.0], [1.0; 3]),
            Marker::new(2, 0, [2.5, 0.0, 0.0], [1.0; 3]),
            Marker::new(3, 0, [10.0, 0.0, 0.0], [1.0; 3]),
        ]],
        vec![
            TrackRow::new(1, 0, 0, 0),
            TrackRow::new(2, 0, 0, 0),
            TrackRow::new(3, 0, 0, 0),
        ],
    )
    .expect("the reference series is consistent");
    let set = TrackSet::new(
        1.0,
        vec![
            Track::new(
                TrackId::new(1),
                vec![TrackNode::new(0, ObjectId::new(1), [1.0, 0.0, 0.0])],
            ),
            Track::new(
                TrackId::new(2),
                vec![TrackNode::new(0, ObjectId::new(2), [-2.0, 0.0, 0.0])],
            ),
        ],
    );
    let matching = Matching::of(&truth, &Graph::of_tracks(&set), 3.0);
    assert_eq!(
        matching.matched(),
        2,
        "the maximum pairing matches both markers inside the tolerance"
    );
    assert_eq!(matching.computed_of_truth(0), Some(1));
    assert_eq!(matching.computed_of_truth(1), Some(0));
    assert_eq!(matching.computed_of_truth(2), None);
    assert_eq!(matching.truth_of_computed(0), Some(1));
    assert_eq!(matching.truth_of_computed(1), Some(0));
}
