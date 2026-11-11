use crate::TrackSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackLayout {
    order: Vec<usize>,
    lanes: Vec<usize>,
}

impl TrackLayout {
    pub fn of(set: &TrackSet) -> Self {
        let mut roots = (0..set.len())
            .filter(|index| set.parents(*index).next().is_none())
            .collect::<Vec<_>>();
        roots.reverse();
        let mut seen = vec![false; set.len()];
        let mut order = Vec::with_capacity(set.len());
        let mut stack = roots;
        while let Some(index) = stack.pop() {
            if std::mem::replace(&mut seen[index], true) {
                continue;
            }
            order.push(index);
            let mut children = set.children(index).collect::<Vec<_>>();
            children.reverse();
            stack.extend(children);
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
