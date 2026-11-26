use grana_object::{Link, SolvedConfidence, Track, TrackId, TrackLink, TrackNode, TrackSet};

use super::chain::Chains;
use super::detection::Detections;

pub(super) fn build(chains: &Chains, detections: &Detections, frame_interval: f32) -> TrackSet {
    let count = chains.len();
    let mut order = (0..count).collect::<Vec<_>>();
    order.sort_unstable_by_key(|index| {
        (
            chains.chain(*index).start(),
            chains.points(*index)[0],
            *index,
        )
    });
    let mut slot = vec![0usize; count];
    for (position, &index) in order.iter().enumerate() {
        slot[index] = position;
    }
    let tracks = order
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let id = TrackId::FIRST
                .after(position)
                .expect("a tracked chain lies inside the id space");
            Track::new(
                id,
                chains
                    .points(index)
                    .iter()
                    .map(|&point| {
                        TrackNode::new(
                            detections.frame(point),
                            detections.id(point),
                            detections.centroid(point).to_array(),
                        )
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let mut incoming = vec![0u8; count];
    let mut outgoing = vec![0u8; count];
    for link in chains.links() {
        outgoing[link.parent] += 1;
        incoming[link.child] += 1;
    }
    let links = chains
        .links()
        .iter()
        .map(|link| {
            let kind = if outgoing[link.parent] == 2 {
                Link::Division
            } else if incoming[link.child] == 2 {
                Link::Merge
            } else {
                Link::Gap
            };
            TrackLink::solved(
                TrackId::FIRST
                    .after(slot[link.parent])
                    .expect("a tracked chain lies inside the id space"),
                TrackId::FIRST
                    .after(slot[link.child])
                    .expect("a tracked chain lies inside the id space"),
                kind,
                SolvedConfidence::new(link.confidence)
                    .expect("the solver writes a calibrated probability"),
            )
        })
        .collect::<Vec<_>>();
    TrackSet::try_with_links(frame_interval, tracks, links)
        .expect("the solved chains form a track set")
}
