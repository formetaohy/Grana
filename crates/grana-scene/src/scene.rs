use std::sync::Arc;

use glam::Vec3;
use grana_display::{Coloring, Shading, TransferFunction};
use grana_geometry::ray_hit;
use grana_measure::{Aggregate, Classifier, Frame, Group, Measurement, RelationRow, Table};
use grana_object::{RelationFrame, SurfaceEdit, TrackSet};
use grana_object::{Shape, TrackParams};
use grana_optics::PsfParams;
use grana_volume::{CHANNEL_LIMIT, ChannelInfo, Clip, Intensity, Region};

use crate::annotation::Annotations;
use crate::channel::Channel;
use crate::coloc::{Coloc, MaskKey};
use crate::color::ObjectColor;
use crate::curation::Curation;
use crate::dataset::Dataset;
use crate::derived::{Derived, DerivedSpec, MaskChannel, MaskFrames, MaskSpec, RestoreSpec};
use crate::drift::Drift;
use crate::key::ObjectSetKey;
use crate::mask::MaskFilter;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId};
use crate::objects::Objects;
use crate::provenance::{Criteria, Provenance};
use crate::segment::SegmentParams;
use crate::state::{HASH_OFFSET, mix};

fn aggregate_scores(
    set: &ObjectSet,
    classifier: &Classifier,
    aggregate: Aggregate,
    group: Group,
) -> Vec<f32> {
    match group {
        Group::Object => Vec::new(),
        Group::Track => {
            let Some(tracks) = set.tracks().map(|layer| layer.set()) else {
                return Vec::new();
            };
            let mut accumulators = vec![grana_object::SampleAccumulator::new(); tracks.len()];
            for (time, frame) in set.frames() {
                let objects = frame.objects();
                for index in 0..frame.object_count() {
                    let Some(track) = tracks.track_at(time, index) else {
                        continue;
                    };
                    let score = classifier.score(objects, index);
                    if score.is_finite() {
                        accumulators[track].push(score);
                    }
                }
            }
            accumulators
                .iter()
                .map(|accumulator| aggregate.apply(accumulator.statistics()))
                .collect()
        }
        Group::Timepoint => set
            .frames()
            .map(|(_, frame)| {
                let objects = frame.objects();
                let mut accumulator = grana_object::SampleAccumulator::new();
                for index in 0..frame.object_count() {
                    let score = classifier.score(objects, index);
                    if score.is_finite() {
                        accumulator.push(score);
                    }
                }
                aggregate.apply(accumulator.statistics())
            })
            .collect(),
    }
}

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
    pub clip: Clip,
    pub mask: Option<ObjectSetId>,
    pub mask_radius: f32,
    pub mask_filter: Option<MaskFilter>,
    pub annotations: Annotations,
    drift: Drift,
    pub objects: Objects,
    pub coloc: Coloc,
    derived: Vec<Derived>,
    base_channels: usize,
    time: u32,
    selections: Vec<Selection>,
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
            clip: Clip::none(),
            mask: None,
            mask_radius: 0.0,
            mask_filter: None,
            annotations: Annotations::new(),
            drift: Drift::default(),
            objects: Objects::new(),
            coloc: Coloc::new(channel_count, region),
            derived: Vec::new(),
            base_channels: channel_count,
            time: 0,
            selections: Vec::new(),
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
    }

    pub fn base_channels(&self) -> usize {
        self.base_channels
    }

    pub fn derived(&self) -> &[Derived] {
        &self.derived
    }

    pub fn derived_specs(&self) -> Vec<DerivedSpec> {
        self.derived
            .iter()
            .map(|entry| entry.spec().clone())
            .collect()
    }

    pub fn set_derived_layout(&mut self, infos: Vec<ChannelInfo>) -> Result<(), String> {
        let base = self.base_channels;
        if base + infos.len() > CHANNEL_LIMIT {
            return Err(format!(
                "the dataset holds {base} channels and grana displays at most {CHANNEL_LIMIT}"
            ));
        }
        let mut channels = self.dataset.channels()[..base].to_vec();
        channels.extend(infos);
        self.dataset = self.dataset.with_channels(channels);
        let mut previous = std::mem::take(&mut self.channels).into_iter();
        self.channels = self
            .dataset
            .channels()
            .iter()
            .enumerate()
            .map(|(index, info)| match previous.next() {
                Some(channel) if index < base => channel,
                Some(_) | None => Channel::new(
                    info.window(),
                    info.window(),
                    TransferFunction::tinted(info.tint()),
                ),
            })
            .collect();
        self.derived.clear();
        self.coloc.set_channel_count(self.channels.len());
        if self.selected >= self.channels.len() {
            self.selected = 0;
        }
        Ok(())
    }

    pub fn bind_derived(&mut self, specs: Vec<DerivedSpec>) -> Result<(), String> {
        assert!(
            self.derived.is_empty(),
            "a scene binds its derived channels once"
        );
        let mut entries = Vec::with_capacity(specs.len());
        for (index, spec) in specs.into_iter().enumerate() {
            let channel = self.base_channels + index;
            let Some(info) = self.dataset.channels().get(channel) else {
                return Err("a derived channel lies outside the dataset".to_string());
            };
            if info.name() != spec.name() {
                return Err(format!(
                    "the derived channel {} does not match the dataset channel {}",
                    spec.name(),
                    info.name()
                ));
            }
            if spec.source() >= self.base_channels {
                return Err("a derived channel derives from a stored channel".to_string());
            }
            entries.push(match spec {
                DerivedSpec::Mask(spec) => {
                    let Some(set) = self.objects.get(spec.set()) else {
                        return Err(format!(
                            "the mask channel {} names an object set that left the scene",
                            spec.name()
                        ));
                    };
                    Derived::masked(spec, mask_frames(set))
                }
                DerivedSpec::Restore(spec) => Derived::restored(spec),
            });
        }
        self.derived = entries;
        Ok(())
    }

    pub fn add_mask(&mut self, set: ObjectSetId, channel: usize) -> Result<usize, String> {
        if channel >= self.base_channels {
            return Err("a mask channel masks a stored channel".to_string());
        }
        let Some(object) = self.objects.get(set) else {
            return Err("the masked object set is missing from the scene".to_string());
        };
        if object.detected() == 0 {
            return Err(format!("{} detected no objects to mask", object.name()));
        }
        if self.derived.iter().any(|entry| {
            entry
                .spec()
                .mask()
                .is_some_and(|spec| spec.set() == set && spec.channel() == channel)
        }) {
            return Err(format!(
                "{} already masks {}",
                object.name(),
                self.dataset.channels()[channel].name()
            ));
        }
        let color = object.color();
        let frames = mask_frames(object);
        let source = self.dataset.channels()[channel].clone();
        let name = self.mask_name(&source, object);
        let spec = MaskSpec::new(set, channel, name, color);
        let info = spec.info(&source);
        self.push_derived(Derived::masked(spec, frames), info)
    }

    pub fn add_restore(&mut self, spec: RestoreSpec) -> Result<usize, String> {
        if spec.channel() >= self.base_channels {
            return Err("a restored channel restores a stored channel".to_string());
        }
        if self
            .derived
            .iter()
            .any(|entry| entry.spec().name() == spec.name())
        {
            return Err(format!("the scene already holds {}", spec.name()));
        }
        let info = spec.info();
        self.push_derived(Derived::restored(spec), info)
    }

    fn push_derived(&mut self, entry: Derived, info: ChannelInfo) -> Result<usize, String> {
        if self.channels.len() >= CHANNEL_LIMIT {
            return Err(format!(
                "the dataset holds {} channels and grana displays at most {CHANNEL_LIMIT}",
                self.channels.len()
            ));
        }
        let mut channels = self.dataset.channels().to_vec();
        channels.push(info.clone());
        self.dataset = self.dataset.with_channels(channels);
        self.channels.push(Channel::new(
            info.window(),
            info.window(),
            TransferFunction::tinted(info.tint()),
        ));
        self.coloc.set_channel_count(self.channels.len());
        self.derived.push(entry);
        Ok(self.channels.len() - 1)
    }

    pub fn remove_derived(&mut self, channel: usize) -> Result<(), String> {
        if channel < self.base_channels || channel >= self.channels.len() {
            return Err("only a derived channel can be removed".to_string());
        }
        let index = channel - self.base_channels;
        if self
            .objects
            .iter()
            .any(|set| set.channel() >= channel || set.measured_channels() > channel)
        {
            return Err("an object set measures this derived channel".to_string());
        }
        if self.drift().channel() == channel && !self.drift().is_empty() {
            return Err("the drift correction aligns this derived channel".to_string());
        }
        self.derived.remove(index);
        self.channels.remove(channel);
        let infos = self
            .dataset
            .channels()
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != channel)
            .map(|(_, info)| info.clone())
            .collect::<Vec<_>>();
        self.dataset = self.dataset.with_channels(infos);
        self.coloc.remove_channel(channel);
        if self.selected >= channel {
            self.selected = self.selected.saturating_sub(1);
        }
        Ok(())
    }

    pub fn set_restore(
        &mut self,
        channel: usize,
        params: PsfParams,
        iterations: u32,
    ) -> Result<bool, String> {
        if channel < self.base_channels || channel >= self.channels.len() {
            return Err("only a derived channel can be restored".to_string());
        }
        let index = channel - self.base_channels;
        let Some(restore) = self.derived[index].spec().restore().cloned() else {
            return Err("the derived channel masks an object set".to_string());
        };
        if restore.params() == params && restore.iterations() == iterations {
            return Ok(false);
        }
        let spec = RestoreSpec::new(
            restore.channel(),
            restore.name().to_string(),
            restore.tint(),
            restore.intensity(),
            params,
            iterations,
        );
        self.derived[index] = Derived::restored(spec);
        Ok(true)
    }

    pub fn sync_masks(&mut self) {
        for entry in &self.derived {
            let (Some(spec), Some(channel)) = (entry.spec().mask(), entry.mask()) else {
                continue;
            };
            let Some(set) = self.objects.get(spec.set()) else {
                continue;
            };
            if mask_frames_current(channel, set) {
                continue;
            }
            channel.set_frames(mask_frames(set));
        }
    }

    pub fn mask_stamp(&self, time: u32) -> u64 {
        let mut stamp = HASH_OFFSET;
        for entry in &self.derived {
            let (Some(spec), Some(channel)) = (entry.spec().mask(), entry.mask()) else {
                continue;
            };
            stamp = mix(stamp, spec.set().raw());
            stamp = mix(stamp, spec.channel() as u64);
            stamp = mix(stamp, Arc::as_ptr(channel) as u64);
            stamp = mix(
                stamp,
                self.objects
                    .get(spec.set())
                    .and_then(|set| set.frame_shared(time))
                    .map_or(0, |frame| Arc::as_ptr(&frame) as u64),
            );
        }
        stamp
    }

    fn mask_name(&self, source: &ChannelInfo, object: &ObjectSet) -> String {
        self.unique_channel_name(&format!("{} in {}", source.name(), object.name()))
    }

    pub fn unique_channel_name(&self, base: &str) -> String {
        if !self
            .dataset
            .channels()
            .iter()
            .any(|channel| channel.name() == base)
        {
            return base.to_string();
        }
        (2..)
            .map(|ordinal| format!("{base} {ordinal}"))
            .find(|candidate| {
                !self
                    .dataset
                    .channels()
                    .iter()
                    .any(|channel| channel.name() == candidate)
            })
            .expect("a derived channel name is unique")
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
            .selections
            .iter()
            .any(|selection| matches!(selection.item, Item::Object(_)))
        {
            self.selections
                .retain(|selection| matches!(selection.item, Item::Track(_)));
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

    pub fn add_segment(&mut self, channel: usize, params: SegmentParams) -> ObjectSetId {
        self.objects.segment(params, channel, &self.dataset)
    }

    pub fn import_object(&mut self, label: &str) -> ObjectSetId {
        self.objects.import(label, self.selected, &self.dataset)
    }

    pub fn import_tracks(&mut self, file: &str) -> ObjectSetId {
        self.objects
            .import_tracks(file, self.selected, &self.dataset)
    }

    pub fn remove_object(&mut self, id: ObjectSetId) {
        self.objects.remove(id);
        self.selections
            .retain(|selection| self.objects.get(selection.set).is_some());
        if self
            .mask
            .is_some_and(|mask| self.objects.get(mask).is_none())
        {
            self.mask = None;
        }
        self.prune_mask_filter();
        self.sync_coloc_inputs();
        self.retain_masks(id);
    }

    pub fn rename_object(&mut self, id: ObjectSetId, name: &str) -> bool {
        self.objects.rename(id, name)
    }

    pub fn set_object_key(&mut self, id: ObjectSetId, key: ObjectSetKey) -> bool {
        self.objects.set_key(id, key)
    }

    pub fn rekey_object(&mut self, id: ObjectSetId) -> bool {
        self.objects.rekey(id)
    }

    pub fn clear_objects(&mut self) {
        self.objects = Objects::new();
        self.mask = None;
        self.mask_filter = None;
        self.selections.clear();
        let _ = self.set_derived_layout(Vec::new());
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
        let mut positions = Vec::with_capacity(keep.len());
        let mut next = 0;
        for kept in keep {
            positions.push(kept.then_some(next));
            if *kept {
                next += 1;
            }
        }
        self.selections.retain_mut(|selection| {
            if selection.set != id {
                return true;
            }
            let Item::Object(index) = selection.item else {
                return true;
            };
            match positions.get(index).copied().flatten() {
                Some(mapped) => {
                    selection.item = Item::Object(mapped);
                    true
                }
                None => false,
            }
        });
        self.sync_coloc_inputs();
        self.sync_masks();
        Ok(true)
    }

    pub fn copy_object(&mut self, id: ObjectSetId) -> Option<ObjectSetId> {
        let copied = self.objects.copy(id);
        if copied.is_some() {
            self.sync_masks();
        }
        copied
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
                Provenance::Filtered { source, .. } | Provenance::Edited { source, .. } => {
                    current = *source;
                }
                Provenance::Detection
                | Provenance::Segmented { .. }
                | Provenance::Imported { .. }
                | Provenance::Interop { .. }
                | Provenance::Copied { .. }
                | Provenance::Merged { .. } => {
                    return Some(current);
                }
            }
        }
    }

    pub fn filtered_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.filtered_dependents(id)
    }

    pub fn edited_dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.edited_dependents(id)
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
        if self
            .derived
            .iter()
            .any(|entry| entry.spec().mask().is_some_and(|spec| spec.set() == source))
        {
            return Err("a mask channel derives from the merged object set".to_string());
        }
        self.objects.merge(source, target)?;
        if self.mask == Some(source) {
            self.mask = None;
        }
        self.selections.retain(|selection| selection.set != source);
        self.prune_mask_filter();
        self.sync_coloc_inputs();
        self.sync_masks();
        Ok(())
    }

    pub fn edited_object(
        &mut self,
        source: ObjectSetId,
        suffix: &str,
        edits: Vec<SurfaceEdit>,
    ) -> Option<ObjectSetId> {
        let edited = self.objects.edited(source, suffix, edits)?;
        self.selections.clear();
        self.sync_coloc_inputs();
        self.sync_masks();
        Some(edited)
    }

    pub fn replan_object(&mut self, id: ObjectSetId, edits: Vec<SurfaceEdit>) -> bool {
        self.objects.replan(id, edits)
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
        if self.mask != source {
            self.mask_filter = None;
        }
        self.mask = source;
        self.sync_coloc_inputs();
    }

    pub fn set_mask_filter(&mut self, filter: Option<MaskFilter>) -> bool {
        match filter {
            Some(filter) => {
                let Some(mask) = self.mask else {
                    return false;
                };
                let Some(set) = self.objects.get(mask) else {
                    return false;
                };
                if self.objects.get(filter.target()).is_none()
                    || set.relation(filter.target(), filter.shift()).is_none()
                {
                    return false;
                }
                if self.mask_filter == Some(filter) {
                    return false;
                }
                self.mask_filter = Some(filter);
            }
            None => {
                if self.mask_filter.is_none() {
                    return false;
                }
                self.mask_filter = None;
            }
        }
        self.sync_coloc_inputs();
        true
    }

    fn prune_mask_filter(&mut self) {
        let Some(filter) = self.mask_filter else {
            return;
        };
        let valid = self.mask.is_some_and(|mask| {
            self.objects.get(mask).is_some() && self.objects.get(filter.target()).is_some()
        });
        if !valid {
            self.mask_filter = None;
        }
    }

    pub fn set_mask_radius(&mut self, radius: f32) -> bool {
        assert!(
            radius.is_finite() && radius >= 0.0,
            "an analysis mask grows by a finite radius"
        );
        if self.mask_radius == radius {
            return false;
        }
        self.mask_radius = radius;
        self.sync_coloc_inputs();
        true
    }

    pub fn set_object_hidden(
        &mut self,
        id: ObjectSetId,
        time: u32,
        objects: &[usize],
        hidden: bool,
    ) -> bool {
        self.objects
            .get_mut(id)
            .is_some_and(|set| set.set_hidden(time, objects, hidden))
    }

    pub fn clear_hidden(&mut self, id: ObjectSetId) -> bool {
        self.objects
            .get_mut(id)
            .is_some_and(ObjectSet::clear_hidden)
    }

    pub fn hidden_count(&self, id: ObjectSetId) -> usize {
        self.objects.get(id).map_or(0, ObjectSet::hidden_count)
    }

    pub fn set_label_metric(&mut self, id: ObjectSetId, metric: Option<ObjectColor>) -> bool {
        self.objects.set_label_metric(id, metric)
    }

    pub fn set_object_coloring(
        &mut self,
        id: ObjectSetId,
        coloring: Coloring<ObjectColor>,
    ) -> bool {
        self.objects.set_object_coloring(id, coloring)
    }

    pub fn object_colors(&self, set: &ObjectSet) -> Option<Vec<[f32; 3]>> {
        let metric = set.object_coloring().metric()?;
        let extent = set.object_extent()?;
        let values = self.objects.metric_values(set.id(), metric, self.time)?;
        let coloring = set.object_coloring();
        Some(
            values
                .iter()
                .map(|value| {
                    if value.is_finite() {
                        coloring.color(extent, *value)
                    } else {
                        set.color()
                    }
                })
                .collect(),
        )
    }

    pub fn label_values(&self, set: &ObjectSet) -> Option<Vec<f32>> {
        let metric = set.label_metric()?;
        self.objects.metric_values(set.id(), metric, self.time)
    }

    pub fn child_sets(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.child_sets(id)
    }

    pub fn table_revision(&self, id: ObjectSetId) -> Option<u64> {
        self.objects.table_revision(id)
    }

    pub fn child_count(&self, parent: ObjectSetId, child: ObjectSetId) -> Option<usize> {
        let query = self.objects.child(parent, child, self.time)?;
        let parents = self.objects.get(parent)?.frame(self.time)?.object_count();
        let values = query.values(parents, grana_measure::ChildField::Count);
        if values.iter().any(|value| !value.is_finite()) {
            return None;
        }
        Some(values.iter().sum::<f32>() as usize)
    }

    pub fn set_region(&mut self, region: Region) {
        if self.region == region {
            return;
        }
        self.region = region;
        self.sync_coloc_inputs();
    }

    pub fn region_scope(&self) -> Option<Region> {
        self.region.enabled().then_some(self.region)
    }

    pub fn set_clip(&mut self, clip: Clip) {
        self.clip = clip;
    }

    pub fn drift(&self) -> &Drift {
        &self.drift
    }

    pub fn set_drift(&mut self, drift: Drift) -> bool {
        if self.drift == drift {
            return false;
        }
        self.drift = drift;
        true
    }

    pub fn mask_frames(&self) -> Result<Option<grana_object::Roi>, String> {
        let Some(set) = self.mask.and_then(|mask| self.objects.get(mask)) else {
            return Ok(None);
        };
        let filtered = set.filtered();
        let filter = self.mask_filter;
        if let Some(filter) = filter
            && set.relation(filter.target(), filter.shift()).is_none()
        {
            return Err(format!(
                "the analysis mask relates {} to an object set that left the scene",
                set.name()
            ));
        }
        let mut frames = Vec::with_capacity(self.frame_count() as usize);
        for time in 0..self.frame_count() {
            let Some(frame) = set.frame_shared(time) else {
                frames.push(None);
                continue;
            };
            let mut keep = (0..frame.object_count())
                .map(|object| !filtered || set.filtered_object(time, object))
                .collect::<Vec<_>>();
            if let Some(filter) = filter
                && keep.iter().any(|kept| *kept)
            {
                let relation = set
                    .relation(filter.target(), filter.shift())
                    .expect("the checked relation is present");
                let target = self
                    .objects
                    .get(filter.target())
                    .map_or("the related set", ObjectSet::name);
                let measured = relation.frame(time).ok_or_else(|| {
                    format!(
                        "the analysis mask needs the relation of {} to {target} at frame {time}",
                        set.name()
                    )
                })?;
                if measured.objects() != frame.object_count() {
                    return Err(format!(
                        "the relation of {} to {target} measures {} objects at frame {time} and the frame holds {}",
                        set.name(),
                        measured.objects(),
                        frame.object_count()
                    ));
                }
                for (object, kept) in keep.iter_mut().enumerate() {
                    *kept &= filter.holds(measured, object);
                }
            }
            frames.push(if keep.iter().all(|kept| *kept) {
                Some(frame)
            } else {
                frame
                    .retain(&keep)
                    .map_err(|error| error.to_string())?
                    .map(Arc::new)
            });
        }
        let frames = Arc::new(frames);
        Ok(Some(
            grana_object::Roi::paged(frames.len(), move |time| shape_at(&frames, time))
                .with_radius(self.mask_radius),
        ))
    }

    pub fn relation_gate(&self, source: ObjectSetId, target: ObjectSetId) -> f32 {
        let voxel = |set: ObjectSetId| {
            let set = self.objects.get(set)?;
            let (_, frame) = set.frames().next()?;
            let spacing = self.dataset.pyramid().level(frame.level()).spacing();
            Some(spacing.x().min(spacing.y()).min(spacing.z()))
        };
        [source, target]
            .into_iter()
            .filter_map(voxel)
            .fold(0.0, f32::max)
            .max(self.dataset.voxel_size().min_element())
    }

    pub fn add_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) -> bool {
        self.add_relation_with_key(
            source,
            target,
            shift,
            crate::relation::key_of(target, shift),
        )
    }

    pub fn add_relation_with_key(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        key: String,
    ) -> bool {
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
        self.objects
            .add_relation_with_key(source, target, shift, key)
    }

    pub fn move_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        from: i32,
        to: i32,
    ) -> bool {
        if source == target && to == 0 {
            return false;
        }
        let frame_count = self.frame_count();
        if (i64::from(to).unsigned_abs() as usize) >= frame_count as usize {
            return false;
        }
        if self
            .objects
            .get(source)
            .and_then(|set| set.relation(target, from))
            .is_none()
        {
            return false;
        }
        self.objects.move_relation(source, target, from, to)
    }

    pub fn remove_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    ) -> bool {
        let removed = self.objects.remove_relation(source, target, shift);
        if removed {
            if self.mask == Some(source)
                && self
                    .mask_filter
                    .is_some_and(|filter| filter.target() == target && filter.shift() == shift)
            {
                self.mask_filter = None;
            }
            self.prune_mask_filter();
            self.sync_coloc_inputs();
        }
        removed
    }

    pub fn install_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        time: u32,
        frame: RelationFrame,
    ) {
        self.objects
            .install_relation(source, target, shift, time, frame);
    }

    pub fn clear_relations(&mut self, id: ObjectSetId) {
        self.objects.clear_relations(id);
        self.prune_mask_filter();
        self.sync_coloc_inputs();
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
                RelationRow::new(
                    relation.key(),
                    target,
                    relation.shift(),
                    relation.frame(time),
                )
            })
            .collect()
    }

    pub fn table(&self, id: ObjectSetId, group: Group, aggregate: Aggregate) -> Option<Table> {
        let object = self.objects.get(id)?;
        let relations = (0..object.frame_count())
            .map(|time| self.relation_rows(id, time))
            .collect::<Vec<_>>();
        let children = match group {
            Group::Object => self.objects.child_sets(id),
            Group::Track | Group::Timepoint => Vec::new(),
        };
        let mut per_time = Vec::with_capacity(object.frame_count() as usize);
        for time in 0..object.frame_count() {
            per_time.push(
                children
                    .iter()
                    .filter_map(|child| self.objects.child(id, *child, time))
                    .collect::<Vec<_>>(),
            );
        }
        let frames = object
            .frames()
            .map(|(time, frame)| {
                Frame::new(
                    time,
                    frame.objects(),
                    &relations[time as usize],
                    &per_time[time as usize],
                )
            })
            .collect::<Vec<_>>();
        if frames.is_empty() {
            return None;
        }
        assert!(
            object.measured_channels() <= self.dataset.channels().len(),
            "an object set measures the channels of the dataset"
        );
        let channels = self.dataset.channels()[..object.measured_channels()]
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let tracks = object.tracks().map(|layer| layer.set());
        let mut table = Table::build(
            &frames,
            tracks,
            &channels,
            self.dataset.frame_interval(),
            group,
            aggregate,
        );
        if let Some(classifier) = object.classifier() {
            let keys = grana_measure::channel_keys(&channels);
            let column = match group {
                Group::Object => {
                    let scores = classifier.scores(&table, &keys);
                    table = table.with_column(classifier.column(), scores);
                    None
                }
                Group::Track => Some((
                    classifier.aggregate_column(aggregate),
                    aggregate_scores(object, classifier, aggregate, Group::Track),
                )),
                Group::Timepoint => Some((
                    classifier.aggregate_column(aggregate),
                    aggregate_scores(object, classifier, aggregate, Group::Timepoint),
                )),
            };
            if let Some((column, values)) = column {
                table = table.with_column(column, values);
            }
        }
        Some(table)
    }

    pub fn train_classifier(
        &mut self,
        id: ObjectSetId,
        features: &[Measurement],
        positives: &[usize],
    ) -> Result<(), String> {
        if features.is_empty() {
            return Err("a classifier trains on at least one measurement".to_string());
        }
        let set = self
            .objects
            .get(id)
            .ok_or("the object set left the scene before it classified")?;
        let colorable = set.colorable_measurements();
        if !features.iter().all(|feature| colorable.contains(feature)) {
            return Err("a classifier trains on measurements of its object set".to_string());
        }
        let table = self
            .table(id, Group::Object, Aggregate::Mean)
            .ok_or("the object set holds no measurements to train on")?;
        let keys = grana_measure::channel_keys(
            &self
                .dataset
                .channels()
                .iter()
                .map(|channel| channel.name().to_string())
                .collect::<Vec<_>>(),
        );
        let classifier = Classifier::train(&table, &keys, features, positives)?;
        self.objects.set_classifier(id, Some(classifier));
        Ok(())
    }

    pub fn clear_classifier(&mut self, id: ObjectSetId) -> bool {
        self.objects.set_classifier(id, None)
    }

    pub fn install_classifier(&mut self, id: ObjectSetId, classifier: Classifier) -> bool {
        self.objects.set_classifier(id, Some(classifier))
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
            self.selections.retain(|selection| {
                !(selection.set == id && matches!(selection.item, Item::Object(_)))
            });
        }
        self.sync_coloc_inputs();
        self.sync_masks();
    }

    pub fn clear_object(&mut self, id: ObjectSetId) {
        self.objects.clear(id);
        self.objects.clear_relations(id);
        self.selections.retain(|selection| selection.set != id);
        self.sync_coloc_inputs();
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        if self.objects.install_tracks(id, set) {
            self.drop_track_selection(id);
        }
    }

    pub fn apply_curation(&mut self, id: ObjectSetId) -> usize {
        let applied = self.objects.apply_curation(id);
        self.sync_masks();
        applied
    }

    pub fn reapply_hidden(&mut self, id: ObjectSetId) -> usize {
        let applied = self.objects.reapply_hidden(id);
        self.sync_masks();
        applied
    }

    pub fn set_curation(&mut self, id: ObjectSetId, curation: Curation) -> bool {
        let changed = self.objects.set_curation(id, curation);
        if changed {
            self.sync_masks();
        }
        changed
    }

    fn retain_masks(&mut self, set: ObjectSetId) {
        if !self
            .derived
            .iter()
            .any(|entry| entry.spec().mask().is_some_and(|spec| spec.set() == set))
        {
            return;
        }
        let specs = self
            .derived_specs()
            .into_iter()
            .filter(|spec| spec.mask().is_none_or(|mask| mask.set() != set))
            .collect::<Vec<_>>();
        let infos = specs
            .iter()
            .filter_map(|spec| {
                self.dataset
                    .channels()
                    .get(spec.source())
                    .map(|source| spec.info(source))
            })
            .collect::<Vec<_>>();
        let _ = self.set_derived_layout(infos);
        let _ = self.bind_derived(specs);
        self.sync_masks();
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

    pub fn surface_hit(&self, origin: Vec3, direction: Vec3) -> Result<Option<Vec3>, String> {
        let mut nearest: Option<Vec3> = None;
        for set in self.pickable() {
            let Some(frame) = set.frame(self.time) else {
                continue;
            };
            let mesh = frame.geometry().map_err(|error| error.to_string())?;
            let Some(hit) = mesh.and_then(|mesh| ray_hit(&mesh, origin, direction)) else {
                continue;
            };
            if nearest.is_none_or(|current| {
                (hit - origin).length_squared() < (current - origin).length_squared()
            }) {
                nearest = Some(hit);
            }
        }
        Ok(nearest)
    }

    fn drop_track_selection(&mut self, id: ObjectSetId) {
        self.selections
            .retain(|selection| !(selection.set == id && matches!(selection.item, Item::Track(_))));
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selections.last().copied()
    }

    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    pub fn select(&mut self, selection: Option<Selection>) {
        self.selections.clear();
        self.selections.extend(selection);
    }

    pub fn toggle_select(&mut self, selection: Selection) {
        match self.selections.iter().position(|held| *held == selection) {
            Some(position) => {
                self.selections.remove(position);
            }
            None => self.selections.push(selection),
        }
    }

    pub fn select_track(&mut self, id: ObjectSetId, index: usize) {
        assert!(
            self.objects
                .get(id)
                .and_then(ObjectSet::tracks)
                .is_some_and(|layer| index < layer.set().len()),
            "the selected track exists"
        );
        self.select(Some(Selection {
            set: id,
            item: Item::Track(index),
        }));
    }

    pub fn selected_track(&self, id: ObjectSetId) -> Option<usize> {
        match self.selection() {
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
        set.visible()
            && (set.provenance().is_merged()
                || set.provenance().is_external()
                || self.channels[set.channel()].enabled)
    }

    pub fn object_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selections.contains(&Selection {
            set,
            item: Item::Object(index),
        })
    }

    pub fn track_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selections.contains(&Selection {
            set,
            item: Item::Track(index),
        })
    }

    fn mask_key(&self) -> Option<MaskKey> {
        let set = self.objects.get(self.mask?)?;
        Some(MaskKey::new(
            set.id(),
            set.document(),
            self.mask_radius,
            self.mask_filter,
        ))
    }

    fn sync_coloc_inputs(&mut self) {
        let mask = self.mask_key();
        self.coloc.set_inputs(self.region, mask);
    }
}

fn mask_frames(set: &ObjectSet) -> MaskFrames {
    Arc::new(
        (0..set.frame_count())
            .map(|time| set.frame_shared(time))
            .collect(),
    )
}

fn mask_frames_current(channel: &MaskChannel, set: &ObjectSet) -> bool {
    let held = channel.frames();
    held.len() == set.frame_count() as usize
        && (0..set.frame_count()).all(|time| {
            match (
                held.get(time as usize).and_then(Option::as_ref),
                set.frame_shared(time),
            ) {
                (Some(held), Some(latest)) => Arc::ptr_eq(held, &latest),
                (None, None) => true,
                _ => false,
            }
        })
}
