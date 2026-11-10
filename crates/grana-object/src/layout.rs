use crate::TrackSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackLayout {
    order: Vec<usize>,
    lanes: Vec<usize>,
}

impl TrackLayout {
    pub fn of(set: &TrackSet) -> Self {
        let mut children = vec![Vec::new(); set.len()];
        let mut roots = Vec::new();
        for index in 0..set.len() {
            match set.track(index).and_then(|track| track.parent()) {
                Some(parent) => children[parent as usize].push(index),
                None => roots.push(index),
            }
        }
        let mut order = Vec::with_capacity(set.len());
        let mut stack = roots.iter().rev().copied().collect::<Vec<_>>();
        while let Some(index) = stack.pop() {
            order.push(index);
            for child in children[index].iter().rev() {
                stack.push(*child);
            }
        }
        assert_eq!(
            order.len(),
            set.len(),
            "every track lies in the lineage forest"
        );
        let mut lanes = vec![usize::MAX; set.len()];
        for (lane, index) in order.iter().enumerate() {
            lanes[*index] = lane;
        }
        Self { order, lanes }
    }

    pub fn order(&self) -> &[usize] {
        &self.order
    }

    pub fn lane(&self, track: usize) -> usize {
        self.lanes[track]
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}
