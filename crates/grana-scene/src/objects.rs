use grana_object::{RelationFrame, TrackSet};

use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId, take_stamp};
use crate::slug::slug;
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
    stamp: u64,
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
            stamp: 0,
        }
    }

    pub fn add(&mut self, kind: ObjectKind, channel: usize, dataset: &Dataset) -> ObjectSetId {
        assert!(
            channel < dataset.channels().len(),
            "every object set names an existing channel"
        );
        let id = ObjectSetId::new(self.next_id);
        self.next_id += 1;
        let mut ordinal = self.ordinals[kind.index()] + 1;
        while self.occupied(&format!("{} {ordinal}", kind.name()), None) {
            ordinal += 1;
        }
        self.ordinals[kind.index()] = ordinal;
        let color = PALETTE[(id.raw() as usize - 1) % PALETTE.len()];
        let name = format!("{} {ordinal}", kind.name());
        self.sets
            .push(ObjectSet::new(id, kind, name, channel, color, dataset));
        id
    }

    pub fn remove(&mut self, id: ObjectSetId) {
        self.sets.retain(|set| set.id() != id);
        self.clear_relations(id);
    }

    pub fn rename(&mut self, id: ObjectSetId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.occupied(name, Some(id)) {
            return false;
        }
        self.get_mut(id).is_some_and(|set| set.set_name(name))
    }

    fn occupied(&self, name: &str, except: Option<ObjectSetId>) -> bool {
        let key = slug(name).to_ascii_lowercase();
        self.sets
            .iter()
            .any(|set| Some(set.id()) != except && slug(set.name()).to_ascii_lowercase() == key)
    }

    pub fn delete_object(&mut self, id: ObjectSetId, time: u32, object: usize) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        if !self
            .get_mut(id)
            .is_some_and(|set| set.delete_object(time, object, stamp))
        {
            return false;
        }
        let targets = self
            .sets
            .iter()
            .filter(|set| set.id() != id && set.relation(id).is_some())
            .map(ObjectSet::id)
            .collect::<Vec<_>>();
        for target in targets {
            let stamp = take_stamp(&mut self.stamp);
            if let Some(set) = self.get_mut(target) {
                set.invalidate_relation(id, time, stamp);
            }
        }
        true
    }

    pub fn add_relation(&mut self, id: ObjectSetId, target: ObjectSetId) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.add_relation(target, stamp))
    }

    pub fn remove_relation(&mut self, id: ObjectSetId, target: ObjectSetId) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.remove_relation(target, stamp))
    }

    pub fn install_relation(
        &mut self,
        id: ObjectSetId,
        target: ObjectSetId,
        time: u32,
        frame: RelationFrame,
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install_relation(target, time, frame, stamp);
        true
    }

    pub fn clear_relations(&mut self, id: ObjectSetId) {
        let stamp = take_stamp(&mut self.stamp);
        for set in &mut self.sets {
            if set.id() == id {
                set.clear_relations(stamp);
            } else {
                set.remove_relation(id, stamp);
            }
        }
    }

    pub fn install(&mut self, id: ObjectSetId, time: u32, frame: ObjectFrame) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install(time, frame, stamp);
        true
    }

    pub fn clear(&mut self, id: ObjectSetId) {
        let stamp = take_stamp(&mut self.stamp);
        if let Some(set) = self.get_mut(id) {
            set.clear(stamp);
        }
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, tracks: TrackSet) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install_tracks(tracks, stamp);
        true
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        let stamp = take_stamp(&mut self.stamp);
        if let Some(set) = self.get_mut(id) {
            set.clear_tracks(stamp);
        }
    }

    pub fn cut_track(&mut self, id: ObjectSetId, time: u32, object: usize) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.cut_track(time, object, stamp))
    }

    pub fn join_tracks(
        &mut self,
        id: ObjectSetId,
        source: (u32, usize),
        target: (u32, usize),
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.join_tracks(source, target, stamp))
    }

    pub fn set_track_thickness(&mut self, id: ObjectSetId, thickness: f32) {
        let stamp = take_stamp(&mut self.stamp);
        if let Some(set) = self.get_mut(id) {
            set.set_track_thickness(thickness, stamp);
        }
    }

    pub(crate) fn restore(&mut self, states: Vec<SetState>, dataset: &Dataset) {
        self.sets.clear();
        self.next_id = 1;
        self.ordinals = [0; 3];
        for state in states {
            assert!(state.id >= self.next_id, "object set ids ascend from one");
            self.next_id = state.id + 1;
            self.ordinals[state.kind.index()] += 1;
            self.sets
                .push(ObjectSet::restore(state, dataset, &mut self.stamp));
        }
        for set in &self.sets {
            for relation in set.relations() {
                assert!(
                    self.sets
                        .iter()
                        .any(|target| target.id() == relation.target()),
                    "a relation names an object set of the scene"
                );
                assert!(
                    relation.target() != set.id(),
                    "a relation names another object set"
                );
            }
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
