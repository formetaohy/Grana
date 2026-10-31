use crate::dataset::Dataset;
use crate::object::ObjectKind;
use crate::object_set::{ObjectSet, ObjectSetId};
use crate::state::SetState;

const PALETTE: [[f32; 3]; 8] = [
    [0.35, 0.70, 1.00],
    [0.35, 0.90, 0.55],
    [1.00, 0.55, 0.35],
    [0.80, 0.55, 1.00],
    [1.00, 0.80, 0.35],
    [0.35, 0.90, 0.90],
    [1.00, 0.45, 0.70],
    [0.75, 0.85, 0.45],
];

pub struct Objects {
    sets: Vec<ObjectSet>,
    next_id: u64,
    ordinals: [u64; 3],
}

impl Default for Objects {
    fn default() -> Self {
        Self::new()
    }
}

impl Objects {
    pub fn new() -> Self {
        Self {
            sets: Vec::new(),
            next_id: 1,
            ordinals: [0; 3],
        }
    }

    pub fn add(&mut self, kind: ObjectKind, channel: usize, dataset: &Dataset) -> ObjectSetId {
        assert!(
            channel < dataset.channels().len(),
            "every object set names an existing channel"
        );
        let id = ObjectSetId::new(self.next_id);
        self.next_id += 1;
        let ordinal = self.ordinals[kind.index()] + 1;
        self.ordinals[kind.index()] = ordinal;
        let color = PALETTE[(id.raw() as usize - 1) % PALETTE.len()];
        let name = format!("{} {ordinal}", kind.name());
        self.sets
            .push(ObjectSet::new(id, kind, name, channel, color, dataset));
        id
    }

    pub fn remove(&mut self, id: ObjectSetId) {
        self.sets.retain(|set| set.id() != id);
    }

    pub(crate) fn restore(&mut self, states: Vec<SetState>, dataset: &Dataset) {
        self.sets.clear();
        self.next_id = 1;
        self.ordinals = [0; 3];
        for state in states {
            assert!(state.id >= self.next_id, "object set ids ascend from one");
            self.next_id = state.id + 1;
            self.ordinals[state.kind.index()] += 1;
            self.sets.push(ObjectSet::restore(state, dataset));
        }
    }

    pub fn get(&self, id: ObjectSetId) -> Option<&ObjectSet> {
        self.sets.iter().find(|set| set.id() == id)
    }

    pub fn get_mut(&mut self, id: ObjectSetId) -> Option<&mut ObjectSet> {
        self.sets.iter_mut().find(|set| set.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.sets.iter()
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}
