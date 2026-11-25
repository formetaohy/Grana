use grana_object::{Link, Track, TrackLink, TrackNode, TrackSet};

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
        .map(|&index| {
            Track::new(
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
    for &(parent, child) in chains.links() {
        outgoing[parent] += 1;
        incoming[child] += 1;
    }
    let links = chains
        .links()
        .iter()
        .map(|&(parent, child)| {
            let kind = if outgoing[parent] == 2 {
                Link::Division
            } else if incoming[child] == 2 {
                Link::Merge
            } else {
                Link::Gap
            };
            TrackLink::new(slot[parent] as u32, slot[child] as u32, kind)
        })
        .collect::<Vec<_>>();
    TrackSet::try_with_links(frame_interval, tracks, links)
        .expect("the solved chains form a track set")
}
