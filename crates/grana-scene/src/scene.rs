use std::sync::Arc;

use glam::Vec3;
use grana_display::{Shading, TransferFunction};
use grana_geometry::ray_hit;
use grana_measure::{Aggregate, Frame, Group, RelationRow, Table};
use grana_object::{RelationFrame, TrackSet};
use grana_object::{Shape, TrackParams};
use grana_volume::{ChannelInfo, Intensity, Region};

use crate::channel::Channel;
use crate::coloc::{Coloc, MaskKey};
use crate::curation::Curation;
use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId};
use crate::objects::Objects;
use crate::provenance::{Criteria, Provenance};

fn shape_at(frames: &[Option<Arc<ObjectFrame>>], time: u32) -> Result<Option<Shape>, String> {
    let Some(frame) = frames.get(time as usize).and_then(|frame| frame.as_deref()) else {
        return Ok(None);
    };
    frame.shape().map(Some).map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub set: ObjectSetId,
    pub item: Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Object(usize),
    Track(usize),
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<Channel>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub exposure: f32,
    pub region: Region,
    pub mask: Option<ObjectSetId>,
    pub objects: Objects,
    pub coloc: Coloc,
    time: u32,
    selection: Option<Selection>,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels()
            .iter()
            .map(|channel| {
                Channel::new(
                    channel.window(),
                    channel.window(),
                    TransferFunction::tinted(channel.tint()),
                )
            })
            .collect::<Vec<Channel>>();
        let half = 0.5 * dataset.world_size();
        let channel_count = channels.len();
        let region = Region::full(-half, half);
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            exposure: 1.0,
            region,
            mask: None,
            objects: Objects::new(),
            coloc: Coloc::new(channel_count, region),
            time: 0,
            selection: None,
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn frame_count(&self) -> u32 {
        self.dataset.frame_count()
    }

    pub fn set_time(&mut self, time: u32) {
        assert!(time < self.frame_count(), "the frame lies inside the scene");
        if time == self.time {
            return;
        }
        self.time = time;
        if self
            .selection
            .is_some_and(|selection| matches!(selection.item, Item::Object(_)))
        {
            self.selection = None;
        }
    }

    pub fn select_channel(&mut self, index: usize) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.selected = index;
    }

    pub fn set_window(&mut self, index: usize, start: f32, end: f32) {
        let data = self.dataset.channels()[index].intensity();
        let low = start.min(end).clamp(data.min(), data.max());
        let high = start.max(end).clamp(data.min(), data.max());
        let minimum = (data.span() * 1.0e-3).max(f32::EPSILON);
        let high = if high - low < minimum {
            (low + minimum).min(data.max())
        } else {
            high
        };
        if high - low <= 0.0 {
            return;
        }
        self.channels[index].window = Intensity::new(low, high);
    }

    pub fn reset_window(&mut self, index: usize) {
        self.channels[index].window = self.channels[index].default_window;
    }

    pub fn set_transfer(&mut self, index: usize, transfer: TransferFunction) {
        self.channels[index].set_transfer(transfer);
    }

    pub fn add_object(&mut self, kind: ObjectKind, channel: usize) -> ObjectSetId {
        self.objects.add(kind, channel, &self.dataset)
    }

    pub fn remove_object(&mut self, id: ObjectSetId) {
        self.objects.remove(id);
        if self
            .selection
            .is_some_and(|selection| self.objects.get(selection.set).is_none())
        {
            self.selection = None;
        }
        if self
            .mask
            .is_some_and(|mask| self.objects.get(mask).is_none())
        {
            self.mask = None;
        }
        self.sync_coloc_inputs();
    }

    pub fn rename_object(&mut self, id: ObjectSetId, name: &str) -> bool {
        self.objects.rename(id, name)
    }

    pub fn clear_objects(&mut self) {
        self.objects = Objects::new();
        self.mask = None;
        self.selection = None;
        self.coloc.clear();
        self.sync_coloc_inputs();
    }

    pub fn reorder_object(&mut self, id: ObjectSetId, index: usize) -> bool {
        self.objects.reorder(id, index)
    }

    pub fn delete_object(
        &mut self,
        id: ObjectSetId,
        time: u32,
        object: usize,
    ) -> Result<bool, String> {
        self.delete_objects(id, time, &[object])
    }

    pub fn delete_objects(
        &mut self,
        id: ObjectSetId,
        time: u32,
        objects: &[usize],
    ) -> Result<bool, String> {
        if objects.is_empty() {
            return Ok(false);
        }
        let Some(count) = self
            .objects
            .get(id)
            .and_then(|set| set.frame(time))
            .map(ObjectFrame::object_count)
        else {
            return Ok(false);
        };
        let mut keep = vec![true; count];
        for object in objects {
            assert!(
                *object < keep.len(),
                "a deleted object lies inside its frame"
            );
            keep[*object] = false;
        }
        self.retain_objects(id, time, &keep)
    }

    pub fn retain_objects(
        &mut self,
        id: ObjectSetId,
        time: u32,
        keep: &[bool],
    ) -> Result<bool, String> {
        if !self.objects.retain_objects(id, time, keep)? {
            return Ok(false);
        }
        if let Some(Selection {
            set,
            item: Item::Object(index),
        }) = self.selection
            && set == id
        {
            self.selection = keep
                .get(index)
                .copied()
                .unwrap_or(false)
                .then(|| Selection {
                    set,
                    item: Item::Object(keep[..index].iter().filter(|kept| **kept).count()),
                });
        }
        self.sync_coloc_inputs();
        Ok(true)
    }

    pub fn copy_object(&mut self, id: ObjectSetId) -> Option<ObjectSetId> {
        self.objects.copy(id)
    }

    pub fn filter_object(
        &mut self,
        id: ObjectSetId,
        suffix: &str,
        criteria: Criteria,
    ) -> Option<ObjectSetId> {
        self.objects.filter(id, suffix, criteria)
    }

    pub fn detection_root(&self, id: ObjectSetId) -> Option<ObjectSetId> {
        let mut current = id;
        loop {
            let set = self.objects.get(current)?;
            match set.provenance() {
                Provenance::Filtered { source, .. } => current = *source,
                Provenance::Detection | Provenance::Copied { .. } | Provenance::Merged { .. } => {
                    return Some(current);
                }
            }
        }
    }

    pub fn filtered_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.filtered_dependents(id)
    }

    pub fn refresh_filtered(&mut self, id: ObjectSetId) -> Result<bool, String> {
        let Some(set) = self.objects.get(id) else {
            return Ok(false);
        };
        let Provenance::Filtered { source, criteria } = set.provenance().clone() else {
            return Ok(false);
        };
        let Some(filtered) = self.objects.get(source) else {
            return Err(format!(
                "{} filters an object set that left the scene",
                set.name()
            ));
        };
        let counts = (0..self.frame_count())
            .map(|time| filtered.frame(time).map_or(0, ObjectFrame::object_count))
            .collect::<Vec<_>>();
        let params = filtered.params();
        if counts.iter().all(|count| *count == 0) {
            for time in 0..self.frame_count() {
                self.install_optional(id, time, None);
            }
            if let Some(set) = self.objects.get_mut(id) {
                set.set_params(params);
            }
            return Ok(true);
        }
        let tracks = filtered.tracks().map(|layer| layer.set());
        let table = self
            .table(source, criteria.group(), Aggregate::Mean)
            .ok_or_else(|| {
                format!(
                    "{} holds no objects to filter into {}",
                    filtered.name(),
                    set.name()
                )
            })?;
        let keep = criteria.keep(&table, tracks, &counts)?;
        if keep.iter().flatten().all(|kept| !kept) {
            return Err(format!(
                "the filter of {} keeps no object of {}",
                set.name(),
                filtered.name()
            ));
        }
        let frames = (0..self.frame_count())
            .map(|time| filtered.frame_shared(time))
            .collect::<Vec<_>>();
        for (time, frame) in frames.into_iter().enumerate() {
            let pruned = match frame {
                Some(frame) => frame
                    .retain(&keep[time])
                    .map_err(|error| error.to_string())?,
                None => None,
            };
            self.install_optional(id, time as u32, pruned);
        }
        if let Some(set) = self.objects.get_mut(id) {
            set.set_params(params);
        }
        self.objects.apply_curation(id);
        Ok(true)
    }

    pub fn merge_objects(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
    ) -> Result<(), String> {
        self.objects.merge(source, target)?;
        if self.mask == Some(source) {
            self.mask = None;
        }
        if self
            .selection
            .is_some_and(|selection| selection.set == source)
        {
            self.selection = None;
        }
        self.sync_coloc_inputs();
        Ok(())
    }

    pub fn duplicate_object(&mut self, id: ObjectSetId) -> Option<ObjectSetId> {
        self.copy_object(id)
    }

    pub fn set_mask(&mut self, source: Option<ObjectSetId>) {
        if let Some(source) = source {
            assert!(
                self.objects.get(source).is_some(),
                "an analysis mask names an object set of the scene"
            );
        }
        self.mask = source;
        self.sync_coloc_inputs();
    }

    pub fn set_region(&mut self, region: Region) {
        if self.region == region {
            return;
        }
        self.region = region;
        self.sync_coloc_inputs();
    }

    pub fn mask_frames(&self) -> Option<grana_object::Roi> {
        let set = self.objects.get(self.mask?)?;
        let frames = Arc::new(
            (0..self.frame_count())
                .map(|time| set.frame_shared(time))
                .collect::<Vec<_>>(),
        );
        Some(grana_object::Roi::paged(frames.len(), move |time| {
            shape_at(&frames, time)
        }))
    }

    pub fn add_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) -> bool {
        if source == target && shift == 0 {
            return false;
        }
        let valid = self.objects.get(source).is_some() && self.objects.get(target).is_some();
        if !valid {
            return false;
        }
        let frame_count = self.frame_count();
        if (i64::from(shift).unsigned_abs() as usize) >= frame_count as usize {
            return false;
        }
        self.objects.add_relation(source, target, shift)
    }

    pub fn set_relation_shift(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    ) -> bool {
        if source == target && shift == 0 {
            return false;
        }
        let frame_count = self.frame_count();
        if (i64::from(shift).unsigned_abs() as usize) >= frame_count as usize {
            return false;
        }
        if self
            .objects
            .get(source)
            .and_then(|set| set.relation(target))
            .is_some_and(|relation| relation.shift() == shift)
        {
            return false;
        }
        self.objects.set_relation_shift(source, target, shift)
    }

    pub fn remove_relation(&mut self, source: ObjectSetId, target: ObjectSetId) -> bool {
        self.objects.remove_relation(source, target)
    }

    pub fn install_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        time: u32,
        frame: RelationFrame,
    ) {
        self.objects.install_relation(source, target, time, frame);
    }

    pub fn clear_relations(&mut self, id: ObjectSetId) {
        self.objects.clear_relations(id);
    }

    pub fn relation_rows(&self, id: ObjectSetId, time: u32) -> Vec<RelationRow<'_>> {
        let Some(set) = self.objects.get(id) else {
            return Vec::new();
        };
        set.relations()
            .iter()
            .map(|relation| {
                let target = self
                    .objects
                    .get(relation.target())
                    .map_or("target", ObjectSet::name);
                RelationRow::new(target, relation.shift(), relation.frame(time))
            })
            .collect()
    }

    pub fn table(&self, id: ObjectSetId, group: Group, aggregate: Aggregate) -> Option<Table> {
        let object = self.objects.get(id)?;
        let relations = (0..object.frame_count())
            .map(|time| self.relation_rows(id, time))
            .collect::<Vec<_>>();
        let frames = object
            .frames()
            .map(|(time, frame)| Frame::new(time, frame.objects(), &relations[time as usize]))
            .collect::<Vec<_>>();
        if frames.is_empty() {
            return None;
        }
        let channels = self
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let tracks = object.tracks().map(|layer| layer.set());
        Some(Table::build(
            &frames,
            tracks,
            &channels,
            self.dataset.frame_interval(),
            group,
            aggregate,
        ))
    }

    pub fn install(&mut self, id: ObjectSetId, time: u32, frame: ObjectFrame) {
        if !self.objects.install(id, time, frame) {
            return;
        }
        self.settle_install(id, time);
    }

    pub fn install_shared(&mut self, id: ObjectSetId, time: u32, frame: Arc<ObjectFrame>) {
        if !self.objects.install_shared(id, time, frame) {
            return;
        }
        self.settle_install(id, time);
    }

    pub fn install_optional(&mut self, id: ObjectSetId, time: u32, frame: Option<ObjectFrame>) {
        if !self.objects.install_optional(id, time, frame.map(Arc::new)) {
            return;
        }
        self.settle_install(id, time);
    }

    fn settle_install(&mut self, id: ObjectSetId, time: u32) {
        if time == self.time {
            let stale = self.selection.is_some_and(|selection| {
                selection.set == id && matches!(selection.item, Item::Object(_))
            });
            if stale {
                self.selection = None;
            }
        }
        self.sync_coloc_inputs();
    }

    pub fn clear_object(&mut self, id: ObjectSetId) {
        self.objects.clear(id);
        self.objects.clear_relations(id);
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
        self.sync_coloc_inputs();
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        if self.objects.install_tracks(id, set) {
            self.drop_track_selection(id);
        }
    }

    pub fn apply_curation(&mut self, id: ObjectSetId) -> usize {
        self.objects.apply_curation(id)
    }

    pub fn set_curation(&mut self, id: ObjectSetId, curation: Curation) -> bool {
        self.objects.set_curation(id, curation)
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        self.objects.clear_tracks(id);
        self.drop_track_selection(id);
    }

    pub fn cut_track(&mut self, id: ObjectSetId, time: u32, object: usize) -> bool {
        if !self.objects.cut_track(id, time, object) {
            return false;
        }
        self.drop_track_selection(id);
        true
    }

    pub fn join_tracks(
        &mut self,
        id: ObjectSetId,
        source: (u32, usize),
        target: (u32, usize),
    ) -> bool {
        if !self.objects.join_tracks(id, source, target) {
            return false;
        }
        self.drop_track_selection(id);
        true
    }

    pub fn set_track_thickness(&mut self, id: ObjectSetId, thickness: f32) {
        self.objects.set_track_thickness(id, thickness);
    }

    pub fn set_track_style(
        &mut self,
        id: ObjectSetId,
        params: TrackParams,
        thickness: f32,
    ) -> bool {
        self.objects.set_track_style(id, params, thickness)
    }

    pub fn seeds(&self, id: ObjectSetId) -> &[[f32; 3]] {
        self.objects.get(id).map_or(&[], |set| set.seeds(self.time))
    }

    pub fn seed_source(&self, id: ObjectSetId) -> Option<ObjectSetId> {
        self.objects.get(id).and_then(ObjectSet::seed_source)
    }

    pub fn seed_count(&self, id: ObjectSetId) -> usize {
        match self.seed_source(id) {
            Some(source) => self
                .objects
                .get(source)
                .and_then(|set| set.frame(self.time))
                .map_or(0, ObjectFrame::object_count),
            None => self.seeds(id).len(),
        }
    }

    pub fn add_seed(&mut self, id: ObjectSetId, position: [f32; 3]) -> bool {
        self.objects.add_seed(id, self.time, position)
    }

    pub fn remove_seed(&mut self, id: ObjectSetId, index: usize) -> bool {
        self.objects.remove_seed(id, self.time, index)
    }

    pub fn clear_seeds(&mut self, id: ObjectSetId) -> bool {
        self.objects.clear_seeds(id, self.time)
    }

    pub fn set_seed_source(&mut self, id: ObjectSetId, source: Option<ObjectSetId>) -> bool {
        if let Some(source) = source {
            assert!(
                self.objects.get(source).is_some(),
                "a seed source names an object set of the scene"
            );
            if source == id || self.depends_on(source, id) {
                return false;
            }
        } else if self.seed_source(id).is_none() {
            return false;
        }
        self.objects
            .set_seed_source(id, source, self.frame_count() as usize)
    }

    pub fn depends_on(&self, id: ObjectSetId, ancestor: ObjectSetId) -> bool {
        let mut stack = vec![id];
        let mut seen = Vec::new();
        while let Some(current) = stack.pop() {
            if current == ancestor {
                return true;
            }
            if seen.contains(&current) {
                continue;
            }
            seen.push(current);
            let Some(set) = self.objects.get(current) else {
                continue;
            };
            stack.extend(set.provenance().sources());
            stack.extend(set.seed_source());
        }
        false
    }

    pub fn seeded_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.seeded_dependents(id)
    }

    pub fn seed_frames(&self, id: ObjectSetId) -> Vec<Vec<[f32; 3]>> {
        let Some(set) = self.objects.get(id) else {
            return Vec::new();
        };
        if let Some(frames) = set.manual_seed_frames() {
            return frames.to_vec();
        }
        let Some(source) = set.seed_source() else {
            return Vec::new();
        };
        let Some(source) = self.objects.get(source) else {
            return vec![Vec::new(); self.frame_count() as usize];
        };
        (0..self.frame_count())
            .map(|time| {
                source
                    .frame(time)
                    .map_or_else(Vec::new, ObjectFrame::centroids)
            })
            .collect()
    }

    pub fn set_seed_frames(&mut self, id: ObjectSetId, seeds: Vec<Vec<[f32; 3]>>) -> bool {
        self.objects.set_seed_frames(id, seeds)
    }

    pub fn surface_ray(
        &self,
        id: ObjectSetId,
        origin: Vec3,
        direction: Vec3,
    ) -> Result<Option<Vec3>, String> {
        let Some(frame) = self.objects.get(id).and_then(|set| set.frame(self.time)) else {
            return Ok(None);
        };
        let mesh = frame.geometry().map_err(|error| error.to_string())?;
        Ok(mesh.and_then(|mesh| ray_hit(&mesh, origin, direction)))
    }

    fn drop_track_selection(&mut self, id: ObjectSetId) {
        let stale = self.selection.is_some_and(|selection| {
            selection.set == id && matches!(selection.item, Item::Track(_))
        });
        if stale {
            self.selection = None;
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn select(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    pub fn select_track(&mut self, id: ObjectSetId, index: usize) {
        assert!(
            self.objects
                .get(id)
                .and_then(ObjectSet::tracks)
                .is_some_and(|layer| index < layer.set().len()),
            "the selected track exists"
        );
        self.selection = Some(Selection {
            set: id,
            item: Item::Track(index),
        });
    }

    pub fn selected_track(&self, id: ObjectSetId) -> Option<usize> {
        match self.selection {
            Some(Selection {
                set,
                item: Item::Track(index),
            }) if set == id => Some(index),
            _ => None,
        }
    }

    pub fn geometry_bytes(&self) -> u64 {
        self.objects
            .iter()
            .map(|set| {
                set.frames()
                    .map(|(_, frame)| frame.geometry_bytes())
                    .sum::<u64>()
            })
            .sum()
    }

    pub fn pickable(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.objects.iter().filter(|set| self.shown(set))
    }

    pub fn shown(&self, set: &ObjectSet) -> bool {
        set.visible() && (set.provenance().is_merged() || self.channels[set.channel()].enabled)
    }

    pub fn object_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Object(index),
            })
    }

    pub fn track_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Track(index),
            })
    }

    fn mask_key(&self) -> Option<MaskKey> {
        let set = self.objects.get(self.mask?)?;
        Some(MaskKey::new(set.id(), set.document()))
    }

    fn sync_coloc_inputs(&mut self) {
        let mask = self.mask_key();
        self.coloc.set_inputs(self.region, mask);
    }
}
