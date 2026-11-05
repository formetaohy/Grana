use std::sync::Arc;

use grana_object::{RelationFrame, TrackSet};

use crate::curation::Curation;
use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId, take_stamp};
use crate::provenance::{Criteria, Provenance};
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
        let mut doomed = vec![id];
        let mut index = 0;
        while index < doomed.len() {
            let dependency = doomed[index];
            index += 1;
            for set in &self.sets {
                let depends = set.provenance().depends_on(dependency)
                    || set.seed_source() == Some(dependency);
                if depends && !doomed.contains(&set.id()) {
                    doomed.push(set.id());
                }
            }
        }
        self.sets.retain(|set| !doomed.contains(&set.id()));
        for set in &mut self.sets {
            set.forget_copies(&doomed);
        }
        for dependency in doomed {
            self.clear_relations(dependency);
        }
    }

    pub fn reorder(&mut self, id: ObjectSetId, index: usize) -> bool {
        let Some(position) = self.sets.iter().position(|set| set.id() == id) else {
            return false;
        };
        let target = index.min(self.sets.len() - 1);
        if position == target {
            return false;
        }
        let set = self.sets.remove(position);
        self.sets.insert(target, set);
        true
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

    pub fn retain_objects(
        &mut self,
        id: ObjectSetId,
        time: u32,
        keep: &[bool],
    ) -> Result<bool, String> {
        let stamp = take_stamp(&mut self.stamp);
        let removed = match self.get_mut(id) {
            Some(set) => set.retain_objects(time, keep, stamp)?,
            None => false,
        };
        if !removed {
            return Ok(false);
        }
        let targets = self
            .sets
            .iter()
            .filter(|set| {
                set.id() != id
                    && set
                        .relations()
                        .iter()
                        .any(|relation| relation.target() == id)
            })
            .map(ObjectSet::id)
            .collect::<Vec<_>>();
        for target in targets {
            let stamp = take_stamp(&mut self.stamp);
            if let Some(set) = self.get_mut(target) {
                set.invalidate_relation(id, time, stamp);
            }
        }
        Ok(true)
    }

    pub fn copy(&mut self, id: ObjectSetId) -> Option<ObjectSetId> {
        self.derive(id, "copy", || Provenance::Copied { source: id })
    }

    pub fn filter(
        &mut self,
        id: ObjectSetId,
        suffix: &str,
        criteria: Criteria,
    ) -> Option<ObjectSetId> {
        self.derive(id, suffix, || Provenance::Filtered {
            source: id,
            criteria,
        })
    }

    fn derive(
        &mut self,
        id: ObjectSetId,
        suffix: &str,
        provenance: impl FnOnce() -> Provenance,
    ) -> Option<ObjectSetId> {
        let source = self.get(id)?;
        let derived = ObjectSetId::new(self.next_id);
        let base = format!("{} {suffix}", source.name());
        let mut name = base.clone();
        let mut ordinal = 2;
        while self.occupied(&name, None) {
            name = format!("{base} {ordinal}");
            ordinal += 1;
        }
        let color = source.color();
        let copy = source.duplicate(derived, name, color, provenance());
        let position = self
            .sets
            .iter()
            .position(|set| set.id() == id)
            .expect("the derived object set exists");
        self.next_id += 1;
        self.sets.insert(position + 1, copy);
        Some(derived)
    }

    pub fn merge(&mut self, source: ObjectSetId, target: ObjectSetId) -> Result<(), String> {
        if source == target {
            return Err("an object set merges into another object set".to_string());
        }
        let source_index = self
            .sets
            .iter()
            .position(|set| set.id() == source)
            .ok_or("the absorbed object set is missing from the scene")?;
        let target_index = self
            .sets
            .iter()
            .position(|set| set.id() == target)
            .ok_or("the merged object set is missing from the scene")?;
        if self.sets[source_index].kind() != self.sets[target_index].kind() {
            return Err("an object set merges into a set of its own kind".to_string());
        }
        if self
            .sets
            .iter()
            .any(|set| set.provenance().depends_on(source))
        {
            return Err(
                "an object set that other object sets filter from cannot be merged".to_string(),
            );
        }
        if self.sets[source_index].provenance().depends_on(target) {
            return Err(
                "an object set derived from the merged object set cannot be merged into it"
                    .to_string(),
            );
        }
        let stamp = take_stamp(&mut self.stamp);
        let mut merged = self.sets[target_index].merged(&self.sets[source_index], stamp)?;
        let mut parts = match merged.provenance() {
            Provenance::Merged { parts } => parts.clone(),
            Provenance::Detection | Provenance::Filtered { .. } | Provenance::Copied { .. } => {
                vec![self.sets[target_index].recipe()]
            }
        };
        parts.push(self.sets[source_index].recipe());
        for part in &mut parts {
            part.forget_copied(&[source, target]);
        }
        merged.set_provenance(Provenance::Merged { parts });
        self.sets.remove(source_index);
        let position = if source_index < target_index {
            target_index - 1
        } else {
            target_index
        };
        self.sets[position] = merged;
        for set in &mut self.sets {
            set.forget_copies(&[source]);
        }
        self.clear_relations(source);
        Ok(())
    }

    pub fn filtered_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        let mut ordered = Vec::new();
        let mut frontier = vec![id];
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for set in &self.sets {
                if let Provenance::Filtered { source, .. } = set.provenance()
                    && frontier.contains(source)
                    && !ordered.contains(&set.id())
                {
                    ordered.push(set.id());
                    next.push(set.id());
                }
            }
            frontier = next;
        }
        ordered
    }

    pub fn seeded_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.sets
            .iter()
            .filter(|set| set.seed_source() == Some(id))
            .map(ObjectSet::id)
            .collect()
    }

    pub fn add_relation(&mut self, id: ObjectSetId, target: ObjectSetId, shift: i32) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.add_relation(target, shift, stamp))
    }

    pub fn move_relation(
        &mut self,
        id: ObjectSetId,
        target: ObjectSetId,
        from: i32,
        to: i32,
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.move_relation(target, from, to, stamp))
    }

    pub fn remove_relation(&mut self, id: ObjectSetId, target: ObjectSetId, shift: i32) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.remove_relation(target, shift, stamp))
    }

    pub fn install_relation(
        &mut self,
        id: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        time: u32,
        frame: RelationFrame,
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install_relation(target, shift, time, frame, stamp);
        true
    }

    pub fn clear_relations(&mut self, id: ObjectSetId) {
        let stamp = take_stamp(&mut self.stamp);
        for set in &mut self.sets {
            if set.id() == id {
                set.clear_relations(stamp);
            } else {
                set.remove_relations_to(id, stamp);
            }
        }
    }

    pub fn install(&mut self, id: ObjectSetId, time: u32, frame: ObjectFrame) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install(time, frame, stamp);
        self.invalidate_relations(id, time);
        true
    }

    pub fn install_shared(&mut self, id: ObjectSetId, time: u32, frame: Arc<ObjectFrame>) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install_shared(time, frame, stamp);
        self.invalidate_relations(id, time);
        true
    }

    pub fn install_optional(
        &mut self,
        id: ObjectSetId,
        time: u32,
        frame: Option<Arc<ObjectFrame>>,
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        let Some(set) = self.get_mut(id) else {
            return false;
        };
        set.install_optional(time, frame, stamp);
        self.invalidate_relations(id, time);
        true
    }

    fn invalidate_relations(&mut self, id: ObjectSetId, time: u32) {
        for index in 0..self.sets.len() {
            let stamp = take_stamp(&mut self.stamp);
            let set = &mut self.sets[index];
            if set.id() == id {
                set.invalidate_relations(time, stamp);
            } else {
                set.invalidate_relation(id, time, stamp);
            }
        }
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

    pub fn apply_curation(&mut self, id: ObjectSetId) -> usize {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id).map_or(0, |set| set.apply_curation(stamp))
    }

    pub fn reapply_hidden(&mut self, id: ObjectSetId) -> usize {
        self.get_mut(id).map_or(0, ObjectSet::reapply_hidden)
    }

    pub fn set_curation(&mut self, id: ObjectSetId, curation: Curation) -> bool {
        self.get_mut(id)
            .is_some_and(|set| set.set_curation(curation))
    }

    pub fn cut_track(&mut self, id: ObjectSetId, time: u32, object: usize) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.cut_track(time, object, stamp))
    }

    pub fn add_seed(&mut self, id: ObjectSetId, time: u32, position: [f32; 3]) -> bool {
        self.get_mut(id)
            .is_some_and(|set| set.add_seed(time, position))
    }

    pub fn remove_seed(&mut self, id: ObjectSetId, time: u32, index: usize) -> bool {
        self.get_mut(id)
            .is_some_and(|set| set.remove_seed(time, index))
    }

    pub fn clear_seeds(&mut self, id: ObjectSetId, time: u32) -> bool {
        self.get_mut(id).is_some_and(|set| set.clear_seeds(time))
    }

    pub fn set_seed_source(
        &mut self,
        id: ObjectSetId,
        source: Option<ObjectSetId>,
        frames: usize,
    ) -> bool {
        self.get_mut(id)
            .is_some_and(|set| set.set_seed_source(source, frames))
    }

    pub fn set_seed_frames(&mut self, id: ObjectSetId, seeds: Vec<Vec<[f32; 3]>>) -> bool {
        self.get_mut(id)
            .is_some_and(|set| set.set_seed_frames(seeds))
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

    pub fn set_track_style(
        &mut self,
        id: ObjectSetId,
        params: grana_object::TrackParams,
        thickness: f32,
    ) -> bool {
        let stamp = take_stamp(&mut self.stamp);
        self.get_mut(id)
            .is_some_and(|set| set.set_track_style(params, thickness, stamp))
    }

    pub(crate) fn restore(&mut self, states: Vec<SetState>, dataset: &Dataset) {
        self.sets.clear();
        self.next_id = 1;
        self.ordinals = [0; 3];
        for state in states {
            assert!(state.id > 0, "an object set id is positive");
            assert!(
                !self.sets.iter().any(|set| set.id().raw() == state.id),
                "object set ids are unique"
            );
            self.next_id = self.next_id.max(state.id + 1);
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
                    relation.target() != set.id() || relation.shift() != 0,
                    "a relation names another object set or shifts in time"
                );
            }
        }
        for set in &self.sets {
            for source in set.provenance().sources() {
                assert!(
                    source != set.id(),
                    "an object set never derives from itself"
                );
                assert!(
                    self.sets.iter().any(|candidate| candidate.id() == source),
                    "a provenance names an object set of the scene"
                );
            }
            assert!(
                set.provenance()
                    .parts()
                    .iter()
                    .all(|part| { part.params().kind() == set.kind() }),
                "a merged object set holds parts of its own kind"
            );
        }
        for set in &self.sets {
            let mut source = set.seed_source();
            while let Some(current) = source {
                assert_ne!(current, set.id(), "an object set never seeds from itself");
                assert!(
                    self.sets.iter().any(|candidate| candidate.id() == current),
                    "a seed source names an object set of the scene"
                );
                source = self
                    .sets
                    .iter()
                    .find(|candidate| candidate.id() == current)
                    .and_then(ObjectSet::seed_source);
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
