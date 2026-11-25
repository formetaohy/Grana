use std::collections::HashMap;

use crate::TrackSet;
use crate::identity::TrackId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackLayout {
    order: Vec<TrackId>,
    lanes: HashMap<TrackId, usize>,
}

impl TrackLayout {
    pub fn of(set: &TrackSet) -> Self {
        let mut roots = set
            .tracks()
            .iter()
            .map(|track| track.id())
            .filter(|id| set.parents(*id).next().is_none())
            .collect::<Vec<_>>();
        roots.reverse();
        let mut seen = vec![false; set.len()];
        let mut order = Vec::with_capacity(set.len());
        let mut stack = roots;
        while let Some(id) = stack.pop() {
            let slot = set
                .position(id)
                .expect("the layout names a track of its set");
            if std::mem::replace(&mut seen[slot], true) {
                continue;
            }
            order.push(id);
            let mut children = set.children(id).collect::<Vec<_>>();
            children.reverse();
            stack.extend(children);
        }
        assert_eq!(
            order.len(),
            set.len(),
            "every track lies in the lineage forest"
        );
        let mut lanes = HashMap::with_capacity(order.len());
        for (lane, id) in order.iter().enumerate() {
            lanes.insert(*id, lane);
        }
        Self { order, lanes }
    }

    pub fn order(&self) -> &[TrackId] {
        &self.order
    }

    pub fn lane(&self, track: TrackId) -> usize {
        self.lanes[&track]
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}
