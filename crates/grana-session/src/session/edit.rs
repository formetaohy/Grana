use super::*;

use grana_object::ObjectAnchor;
use grana_scene::{ObjectFrame, ObjectKind, ObjectSet, SurfaceEdit, SurfaceEditKind};

type Deletions = (ObjectSetId, Vec<(u32, Vec<usize>)>);

#[derive(Clone, Debug)]
pub(super) enum SurfaceEditRequest {
    Merge(Vec<usize>),
    Split(usize),
}

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

    pub fn hide_selected(&mut self) -> bool {
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
        let Some(set) = scene.objects.get(selection.set) else {
            return false;
        };
        if !set.visible_object(time, object) {
            return false;
        }
        self.execute(Command::SetObjectHidden {
            set: selection.set,
            time,
            objects: vec![object],
            hidden: true,
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
        self.defer_relations(set);
        self.refresh_dependents(set);
        self.resume_relations(set);
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

    pub(super) fn set_seed_source(
        &mut self,
        set: ObjectSetId,
        source: Option<ObjectSetId>,
    ) -> bool {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay keeps the seeds it staged",
            ));
            return false;
        }
        if self.busy(set) {
            self.message = Some(Message::error("the object set is busy with an analysis"));
            return false;
        }
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let Some(object) = scene.objects.get(set) else {
            return false;
        };
        if object.kind() != grana_scene::ObjectKind::Surfaces {
            self.message = Some(Message::error("only surfaces carry seed points"));
            return false;
        }
        if let Some(source) = source {
            if source == set {
                self.message = Some(Message::error("an object set cannot seed from itself"));
                return false;
            }
            if scene.objects.get(source).is_none() {
                self.message = Some(Message::error("the seed source left the scene"));
                return false;
            }
            if scene.depends_on(source, set) {
                self.message = Some(Message::error(
                    "the seed source already derives from the object set",
                ));
                return false;
            }
        }
        scene.set_seed_source(set, source)
    }

    pub(super) fn extract_filtered(&mut self) -> bool {
        let (source, _) = match self.filtered_keeps(false) {
            Ok(plan) => plan,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return false;
            }
        };
        let criteria = match self.filtered_criteria() {
            Ok(criteria) => criteria,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return false;
            }
        };
        let derived = {
            let Some(scene) = self.scene.as_mut() else {
                return false;
            };
            let Some(derived) = scene.filter_object(source, "filtered", criteria) else {
                self.message = Some(Message::error("the object set cannot be derived"));
                return false;
            };
            if let Err(detail) = scene.refresh_filtered(derived) {
                self.message = Some(Message::error(detail));
                return false;
            }
            scene.clear_tracks(derived);
            derived
        };
        self.defer_relations(derived);
        self.resume_relations(derived);
        self.start_tracks(derived);
        if let Some(scene) = self.scene.as_ref() {
            self.measure.select(derived, scene);
        }
        true
    }

    fn filtered_criteria(&self) -> Result<Criteria, String> {
        let table = self
            .measure
            .table()
            .ok_or("the object set holds no measurement table")?;
        let group = self.measure.group();
        if group == Group::Timepoint {
            return Err("objects cannot be filtered by a timepoint table".to_string());
        }
        let key = |column: usize| {
            table
                .columns()
                .get(column)
                .map(|column| column.key().to_string())
                .ok_or_else(|| "the statistics filter names a missing column".to_string())
        };
        let filters = self
            .measure
            .filters()
            .iter()
            .map(|filter| {
                Ok(FilterRange::new(
                    &key(filter.column())?,
                    filter.minimum(),
                    filter.maximum(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let lasso = match self.measure.lasso() {
            Some(lasso) => Some(
                LassoPath::new(&key(lasso.x())?, &key(lasso.y())?, lasso.points().to_vec())
                    .ok_or_else(|| "the statistics lasso gates a degenerate polygon".to_string())?,
            ),
            None => None,
        };
        Ok(Criteria::new(group, filters, lasso))
    }

    pub(super) fn reload_filtered(&mut self, set: ObjectSetId) -> Result<(), String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no dataset is open".to_string());
        };
        if !scene.refresh_filtered(set)? {
            return Err("the object set holds no statistics filter".to_string());
        }
        scene.clear_tracks(set);
        Ok(())
    }

    pub(super) fn copy_frames(
        &mut self,
        set: ObjectSetId,
        source: ObjectSetId,
    ) -> Result<(), String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no dataset is open".to_string());
        };
        let frames = (0..scene.frame_count())
            .map(|time| {
                scene
                    .objects
                    .get(source)
                    .and_then(|object| object.frame_shared(time))
            })
            .collect::<Vec<_>>();
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                scene.install_shared(set, time as u32, frame);
            }
        }
        scene.apply_curation(set);
        scene.clear_tracks(set);
        Ok(())
    }

    pub(super) fn merge_sets(&mut self, set: ObjectSetId, part: ObjectSetId) -> Result<(), String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no dataset is open".to_string());
        };
        scene.merge_objects(part, set)?;
        if self.measure.set() == Some(part)
            && let Some(scene) = self.scene.as_ref()
        {
            self.measure.select(set, scene);
        }
        Ok(())
    }

    pub(super) fn refresh_dependents(&mut self, root: ObjectSetId) {
        let (filtered, edited, seeded) = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            (
                scene.filtered_dependents(root),
                scene.edited_dependents(root),
                scene.seeded_dependents(root),
            )
        };
        let mut seeded = seeded;
        for set in filtered {
            let refreshed = self.scene.as_mut().map(|scene| scene.refresh_filtered(set));
            match refreshed {
                Some(Ok(true)) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.clear_tracks(set);
                    }
                    self.defer_relations(set);
                    self.resume_relations(set);
                    self.start_tracks(set);
                    if let Some(scene) = self.scene.as_ref() {
                        seeded.extend(scene.seeded_dependents(set));
                    }
                }
                Some(Ok(false)) => {}
                Some(Err(detail)) => {
                    self.message = Some(Message::error(detail));
                    return;
                }
                None => return,
            }
        }
        for set in edited {
            if !self.refresh_edited(set) {
                return;
            }
        }
        seeded.sort_unstable_by_key(|set| set.raw());
        seeded.dedup();
        for set in seeded {
            if self.recipe.is_some() {
                return;
            }
            self.start_detect(set);
        }
    }

    pub(super) fn refresh_edited(&mut self, set: ObjectSetId) -> bool {
        if let Err(detail) = self.apply_edit_plan(set) {
            self.message = Some(Message::error(detail));
            return false;
        }
        self.defer_relations(set);
        self.resume_relations(set);
        self.start_tracks(set);
        true
    }

    pub(super) fn apply_edit_plan(&mut self, set: ObjectSetId) -> Result<(), String> {
        let Some((source, plan, voxel)) = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .and_then(|object| {
                object
                    .provenance()
                    .editing()
                    .map(|(source, edits)| (source, edits.to_vec(), object.voxel()))
            })
        else {
            return Err("the object set carries no surface edits".to_string());
        };
        let frames = {
            let Some(scene) = self.scene.as_ref() else {
                return Err("no dataset is open".to_string());
            };
            (0..scene.frame_count())
                .map(|time| {
                    scene
                        .objects
                        .get(source)
                        .and_then(|object| object.frame_shared(time))
                })
                .collect::<Vec<_>>()
        };
        let edited = grana_pipeline::run::edit(&frames, &plan, voxel, Some(&self.store))?;
        if let Some(scene) = self.scene.as_mut() {
            for (time, frame) in edited.into_iter().enumerate() {
                if let Some(frame) = frame {
                    scene.install_shared(set, time as u32, frame);
                }
            }
            scene.clear_tracks(set);
        }
        Ok(())
    }

    pub(super) fn edit_surfaces(
        &mut self,
        set: ObjectSetId,
        time: u32,
        request: SurfaceEditRequest,
    ) -> bool {
        if self.recipe.is_some() {
            self.message = Some(Message::error("a protocol replay edits no surfaces"));
            return false;
        }
        if self.busy(set) {
            self.message = Some(Message::error("the object set is busy with an analysis"));
            return false;
        }
        let plan = {
            let Some(scene) = self.scene.as_ref() else {
                return false;
            };
            let Some(object) = scene.objects.get(set) else {
                return false;
            };
            match surface_plan(scene, object, time, request) {
                Ok(plan) => plan,
                Err(detail) => {
                    self.message = Some(Message::error(detail));
                    return false;
                }
            }
        };
        let mut created = false;
        let (source, target) = match self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .and_then(|object| {
                object
                    .provenance()
                    .editing()
                    .map(|(source, edits)| (source, edits.to_vec()))
            }) {
            Some((source, mut edits)) => {
                edits.extend(plan);
                let replanned = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.replan_object(set, edits));
                if !replanned {
                    self.message = Some(Message::error("the object set holds no surface edits"));
                    return false;
                }
                (source, set)
            }
            None => {
                let Some(edited) = self
                    .scene
                    .as_mut()
                    .and_then(|scene| scene.edited_object(set, "edited", plan))
                else {
                    self.message = Some(Message::error("the surface edits derive no object set"));
                    return false;
                };
                created = true;
                (set, edited)
            }
        };
        if let Err(detail) = self.apply_edit_plan(target) {
            if created {
                self.remove_object(target);
            }
            self.message = Some(Message::error(detail));
            return false;
        }
        let edited = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(target))
            .map_or(0, |object| {
                let mut frames = object
                    .provenance()
                    .edits()
                    .iter()
                    .map(|edit| edit.frame())
                    .collect::<Vec<_>>();
                frames.sort_unstable();
                frames.dedup();
                frames.len()
            });
        if let Some(source) = self.object_mut(source) {
            source.set_visible(false);
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.select(None);
        }
        self.defer_relations(target);
        self.resume_relations(target);
        self.start_tracks(target);
        if let Some(scene) = self.scene.as_ref() {
            self.measure.select(target, scene);
        }
        self.message = Some(Message::info(format!(
            "edited the surfaces of {edited} frames into a derived object set"
        )));
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
                self.defer_relations(target);
                self.resume_relations(target);
                self.refresh_dependents(target);
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
        self.cancel(Job::Import(set));
        self.cancel(Job::Recut(set));
        self.cancel(Job::Track(set));
        if self.recut.is_some_and(|(pending, _)| pending == set) {
            self.recut = None;
        }
        self.clear_relations(set);
        let masked = self.scene.as_ref().is_some_and(|scene| {
            scene
                .derived()
                .iter()
                .any(|entry| entry.spec().mask().is_some_and(|spec| spec.set() == set))
        });
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_object(set);
        }
        if masked {
            self.after_mask_layout();
        }
    }
}

fn surface_plan(
    scene: &Scene,
    object: &ObjectSet,
    time: u32,
    request: SurfaceEditRequest,
) -> Result<Vec<SurfaceEdit>, String> {
    let (objects, kind) = match request {
        SurfaceEditRequest::Merge(objects) => (objects, SurfaceEditKind::Merge),
        SurfaceEditRequest::Split(object) => (vec![object], SurfaceEditKind::Split),
    };
    let mut unique = objects.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != objects.len() {
        return Err("the surface edit names one surface twice".to_string());
    }
    if kind == SurfaceEditKind::Merge && unique.len() < 2 {
        return Err("merging needs two distinct surfaces".to_string());
    }
    let Some(frame) = object.frame(time) else {
        return Err("the object set holds no surface here".to_string());
    };
    if frame.kind() != ObjectKind::Surfaces {
        return Err("only surfaces can be merged or split".to_string());
    }
    let anchors = frame.anchors();
    let mut recorded = Vec::with_capacity(unique.len());
    for index in &unique {
        let Some(anchor) = anchors.get(*index) else {
            return Err(format!(
                "the frame {time} holds {} surfaces but names {index}",
                anchors.len()
            ));
        };
        recorded.push(*anchor);
    }
    let mut plan = Vec::new();
    for current in 0..scene.frame_count() {
        let resolved = if current == time {
            Some(recorded.clone())
        } else {
            counterparts(object, current, time, &unique)
        };
        let Some(resolved) = resolved else {
            continue;
        };
        if resolved
            .iter()
            .enumerate()
            .any(|(index, anchor)| resolved[..index].contains(anchor))
        {
            continue;
        }
        plan.push(match kind {
            SurfaceEditKind::Merge => SurfaceEdit::merge(current, resolved),
            SurfaceEditKind::Split => SurfaceEdit::split(current, resolved[0]),
        });
    }
    if plan.is_empty() {
        return Err("the edited surfaces share no frame".to_string());
    }
    Ok(plan)
}

fn counterparts(
    object: &ObjectSet,
    current: u32,
    recorded_time: u32,
    objects: &[usize],
) -> Option<Vec<ObjectAnchor>> {
    let frame = object.frame(current)?;
    if frame.kind() != ObjectKind::Surfaces {
        return None;
    }
    let anchors = frame.anchors();
    let tracks = object.tracks()?.set();
    let mut indices = Vec::with_capacity(objects.len());
    for index in objects {
        let track = tracks.track_at(recorded_time, *index)?;
        let node = tracks
            .track(track)?
            .nodes()
            .iter()
            .find(|node| node.time() == current)?;
        indices.push(node.object());
    }
    indices
        .into_iter()
        .map(|index| anchors.get(index).copied())
        .collect()
}
