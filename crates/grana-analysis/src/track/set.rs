use grana_object::{Track, TrackLink, TrackNode, TrackSet};

use super::chain::Chains;
use super::detection::Detections;

pub(super) fn build(chains: &Chains, detections: &Detections, frame_interval: f32) -> TrackSet {
    let mut order = (0..chains.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|&index| (chains.chain(index).start(), index));
    let mut slot = vec![0usize; chains.len()];
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
                            detections.object(point),
                            detections.centroid(point).to_array(),
                        )
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let links = chains
        .links()
        .iter()
        .map(|&(parent, child, kind)| TrackLink::new(slot[parent] as u32, slot[child] as u32, kind))
        .collect::<Vec<_>>();
    TrackSet::try_with_links(frame_interval, tracks, links)
        .expect("the linked chains form a track set")
}
