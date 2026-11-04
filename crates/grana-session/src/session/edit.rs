use super::*;

type Deletions = (ObjectSetId, Vec<(u32, Vec<usize>)>);

impl Session {
    pub fn delete_selected(&mut self) -> bool {
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        let Some(selection) = scene.selection() else {
            return false;
        };
        let Item::Object(object) = selection.item else {
            return false;
        };
        let time = scene.time();
        self.execute(Command::DeleteObject {
            set: selection.set,
            time,
            object,
        });
        true
    }

    pub(super) fn delete_filtered(&mut self) -> bool {
        let (set, deletions) = match self.filtered_deletions() {
            Ok(plan) => plan,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return false;
            }
        };
        let mut removed = 0usize;
        for (time, objects) in deletions {
            let Some(scene) = self.scene.as_mut() else {
                break;
            };
            match scene.delete_objects(set, time, &objects) {
                Ok(true) => removed += objects.len(),
                Ok(false) => {}
                Err(detail) => {
                    self.message = Some(Message::error(detail));
                    return removed > 0;
                }
            }
        }
        if removed == 0 {
            self.message = Some(Message::error(
                "every object matches the statistics filters",
            ));
            return false;
        }
        true
    }

    pub(super) fn filtered_keeps(
        &self,
        scope: bool,
    ) -> Result<(ObjectSetId, Vec<Vec<bool>>), String> {
        let scene = self.scene.as_ref().ok_or("no dataset is open")?;
        let set = self
            .measure
            .set()
            .ok_or("choose an object set before filtering objects")?;
        if self.measure.filters().is_empty() && self.measure.lasso().is_none() {
            return Err("a statistics filter or lasso is needed".to_string());
        }
        if self.measure.group() == Group::Timepoint {
            return Err("objects cannot be filtered by a timepoint table".to_string());
        }
        if self.busy(set) {
            return Err("the object set is busy with an analysis".to_string());
        }
        let table = self
            .measure
            .table()
            .ok_or("the object set holds no measurement table")?;
        let object = scene
            .objects
            .get(set)
            .ok_or("the object set is missing from the scene")?;
        let mut keep = (0..scene.frame_count())
            .map(|time| vec![false; object.frame(time).map_or(0, ObjectFrame::object_count)])
            .collect::<Vec<_>>();
        let rows = if scope {
            self.measure.rows().to_vec()
        } else {
            table.matching(self.measure.filters(), self.measure.lasso(), None)
        };
        match self.measure.group() {
            Group::Object => {
                for row in rows {
                    let observation = table.observation(row);
                    if let (Some(time), Some(index)) = (observation.time(), observation.object())
                        && let Some(mask) = keep.get_mut(time as usize)
                        && let Some(slot) = mask.get_mut(index)
                    {
                        *slot = true;
                    }
                }
            }
            Group::Track => {
                let Some(tracks) = object.tracks().map(|layer| layer.set()) else {
                    return Err("the object set carries no tracks".to_string());
                };
                for row in rows {
                    let Some(track) = table.observation(row).track() else {
                        continue;
                    };
                    let Some(track) = tracks.track(track) else {
                        continue;
                    };
                    for node in track.nodes() {
                        if let Some(mask) = keep.get_mut(node.time() as usize)
                            && let Some(slot) = mask.get_mut(node.object())
                        {
                            *slot = true;
                        }
                    }
                }
            }
            Group::Timepoint => unreachable!("the timepoint group is rejected above"),
        }
        if keep.iter().flatten().all(|kept| !kept) {
            return Err("the statistics filters keep no object".to_string());
        }
        Ok((set, keep))
    }

    pub(super) fn filtered_deletions(&self) -> Result<Deletions, String> {
        let (set, keep) = self.filtered_keeps(true)?;
        let scene = self.scene.as_ref().ok_or("no dataset is open")?;
        let current =
            self.measure.scope() == Scope::Current && self.measure.group() != Group::Track;
        let mut deletions = Vec::new();
        for (time, mask) in keep.iter().enumerate() {
            if current && time != scene.time() as usize {
                continue;
            }
            let objects = (0..mask.len())
                .filter(|object| !mask[*object])
                .collect::<Vec<_>>();
            if !objects.is_empty() {
                deletions.push((time as u32, objects));
            }
        }
        Ok((set, deletions))
    }

    pub(super) fn extract_filtered(&mut self) -> bool {
        let (source, keep) = match self.filtered_keeps(false) {
            Ok(plan) => plan,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return false;
            }
        };
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let Some(derived) = scene.derive_object(source, "filtered") else {
            self.message = Some(Message::error("the object set cannot be derived"));
            return false;
        };
        for (time, mask) in keep.iter().enumerate() {
            if mask.iter().all(|kept| *kept) {
                continue;
            }
            if let Err(detail) = scene.retain_objects(derived, time as u32, mask) {
                self.message = Some(Message::error(detail));
                return false;
            }
        }
        true
    }

    pub(super) fn merge_objects(&mut self, source: ObjectSetId, target: ObjectSetId) -> bool {
        if self.recipe.is_some() {
            self.message = Some(Message::error("a protocol replay merges no object sets"));
            return false;
        }
        if self.busy(source) || self.busy(target) {
            self.message = Some(Message::error("the object set is busy with an analysis"));
            return false;
        }
        let merged = match self.scene.as_mut() {
            Some(scene) => scene.merge_objects(source, target),
            None => return false,
        };
        match merged {
            Ok(()) => {
                if self.measure.set() == Some(source)
                    && let Some(scene) = self.scene.as_ref()
                {
                    self.measure.select(target, scene);
                }
                self.message = None;
                true
            }
            Err(detail) => {
                self.message = Some(Message::error(detail));
                false
            }
        }
    }

    pub(super) fn busy(&self, set: ObjectSetId) -> bool {
        self.analysis
            .iter()
            .any(|analysis| analysis.job().involves(set))
            || self.recut.is_some_and(|(pending, _)| pending == set)
    }

    pub(super) fn remove_object(&mut self, set: ObjectSetId) {
        self.cancel(Job::Detect(set));
        self.cancel(Job::Recut(set));
        self.cancel(Job::Track(set));
        if self.recut.is_some_and(|(pending, _)| pending == set) {
            self.recut = None;
        }
        self.clear_relations(set);
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_object(set);
        }
    }
}
