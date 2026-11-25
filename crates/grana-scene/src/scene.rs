use std::sync::Arc;

use crate::alignment::LocalizationAlignment;
use crate::annotation::Annotations;
use crate::channel::Channel;
use crate::coloc::{Coloc, MaskKey};
use crate::color::ObjectColor;
use crate::curation::Curation;
use crate::dataset::Dataset;
use crate::derived::{Derived, DerivedSpec, Operation, RestoreSpec, mask_info, mask_intensity};
use crate::drift::Drift;
use crate::drifts::Drifts;
use crate::key::ObjectSetKey;
use crate::label_export::LabelExport;
use crate::mask::{MaskFilter, MaskGate, RelationGate};
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId, Readoption};
use crate::objects::Objects;
use crate::overlay::Overlays;
use crate::provenance::{Criteria, Provenance};
use crate::registration::Registration;
use crate::segment::SegmentParams;
use crate::state::{HASH_OFFSET, SetState, mix};
use crate::table_request::TableRequest;
use crate::track_layer::TrackLayer;
use glam::Vec3;
use grana_display::{
    Coloring, Extent, Material, Shading, TransferFunction, VolumeMode, categorical,
};
use grana_geometry::ray_hit;
use grana_measure::{Aggregate, Classifier, Group, Measurement, ObjectColoc, RelationRow, Table};
use grana_object::{
    MaskEdit, ObjectId, ObjectIds, RelationFrame, SurfaceEdit, TrackId, TrackParams, TrackSet,
};
use grana_optics::Psf;
use grana_volume::{
    CHANNEL_LIMIT, Calibration, ChannelInfo, ChannelSet, Clip, Intensity, Region, Spacing,
};

pub type MaskExtent = ([f32; 3], [f32; 3]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub set: ObjectSetId,
    pub item: Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Object(ObjectId),
    Track(TrackId),
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
    drifts: Drifts,
    registration: Registration,
    pub objects: Objects,
    pub overlays: Overlays,
    pub coloc: Coloc,
    derived: Vec<Derived>,
    base_channels: usize,
    time: u32,
    pub(crate) selections: Vec<Selection>,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels()
            .iter()
            .map(|channel| Channel::new(channel.display(), initial_transfer(channel)))
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
            drifts: Drifts::new(),
            registration: Registration::default(),
            objects: Objects::new(),
            overlays: Overlays::default(),
            coloc: Coloc::new(channel_count),
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

    pub fn stored_dataset(&self) -> Dataset {
        self.dataset
            .with_channels(self.dataset.channels()[..self.base_channels].to_vec())
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

    pub fn derived_shared(&self) -> Vec<Arc<DerivedSpec>> {
        self.derived.iter().map(Derived::shared).collect()
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
                Some(_) | None => Channel::new(info.display(), initial_transfer(info)),
            })
            .collect();
        self.derived.clear();
        self.coloc.rebuild_channels(self.channels.len());
        self.clear_object_coloc_from(self.channels.len());
        if self.selected >= self.channels.len() {
            self.selected = 0;
        }
        Ok(())
    }

    fn clear_object_coloc_from(&mut self, channel: usize) {
        let stale = self
            .objects
            .iter()
            .filter(|set| {
                set.object_coloc()
                    .is_some_and(|coloc| coloc.pair().a() >= channel || coloc.pair().b() >= channel)
            })
            .map(|set| set.id())
            .collect::<Vec<_>>();
        for id in stale {
            self.objects.set_object_coloc(id, None);
        }
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
            if self.dataset.channels()[..channel]
                .iter()
                .any(|other| other.name() == spec.name())
            {
                return Err(format!(
                    "the derived channel {} repeats a channel name",
                    spec.name()
                ));
            }
            if spec.source() >= channel {
                return Err(format!(
                    "the derived channel {} reads a channel that follows it",
                    spec.name()
                ));
            }
            if let Some(denominator) = spec.ratio_denominator() {
                if denominator >= channel {
                    return Err(format!(
                        "the ratio channel {} divides a channel that follows it",
                        spec.name()
                    ));
                }
                if denominator == spec.source() {
                    return Err(format!(
                        "the ratio channel {} divides two distinct channels",
                        spec.name()
                    ));
                }
            }
            entries.push(match spec.mask() {
                Some(set) => {
                    let Some(object) = self.objects.get(set) else {
                        return Err(format!(
                            "the mask channel {} names an object set that left the scene",
                            spec.name()
                        ));
                    };
                    let (gate, stamp) = match self.mask_gate(object) {
                        Ok(gate) => (gate, self.gate_stamp(object)),
                        Err(_) => (MaskGate::new(object.plain_source(), None, None), 0),
                    };
                    Derived::masked(spec, gate, self.mask_radius, stamp)
                }
                None => Derived::plain(spec),
            });
        }
        self.derived = entries;
        Ok(())
    }

    pub fn add_mask(&mut self, set: ObjectSetId, channel: usize) -> Result<usize, String> {
        if channel >= self.channels.len() {
            return Err("a mask channel masks a channel of the dataset".to_string());
        }
        let Some(object) = self.objects.get(set) else {
            return Err("the masked object set is missing from the scene".to_string());
        };
        if object.detected() == 0 {
            return Err(format!("{} detected no objects to mask", object.name()));
        }
        if self
            .derived
            .iter()
            .any(|entry| entry.spec().mask() == Some(set) && entry.spec().source() == channel)
        {
            return Err(format!(
                "{} already masks {}",
                object.name(),
                self.dataset.channels()[channel].name()
            ));
        }
        let color = object.color();
        let gate = self.mask_gate(object)?;
        let stamp = self.gate_stamp(object);
        let source = self.dataset.channels()[channel].clone();
        let name = self.mask_name(&source, object);
        let intensity = mask_intensity(&source);
        let spec = DerivedSpec::masked(set, channel, name, color, intensity);
        let info = mask_info(spec.name(), color, &source);
        self.push_derived(Derived::masked(spec, gate, self.mask_radius, stamp), info)
    }

    pub fn add_derived(&mut self, spec: DerivedSpec) -> Result<usize, String> {
        if spec.mask().is_some() {
            return Err("a mask channel binds the objects it masks".to_string());
        }
        if spec.source() >= self.channels.len() {
            return Err("a derived channel reads a channel of the dataset".to_string());
        }
        if let Some(denominator) = spec.ratio_denominator() {
            if denominator >= self.channels.len() {
                return Err("a ratio channel divides a channel of the dataset".to_string());
            }
            if denominator == spec.source() {
                return Err("a ratio channel divides two distinct channels".to_string());
            }
        }
        if self
            .derived
            .iter()
            .any(|entry| entry.spec().name() == spec.name())
        {
            return Err(format!("the scene already holds {}", spec.name()));
        }
        let info = spec.info(self.channel(spec.source()));
        self.push_derived(Derived::plain(spec), info)
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
        self.channels
            .push(Channel::new(info.display(), initial_transfer(&info)));
        self.coloc.grow_channel(self.channels.len());
        self.derived.push(entry);
        Ok(self.channels.len() - 1)
    }

    pub fn remove_derived(&mut self, channel: usize) -> Result<(), String> {
        if channel < self.base_channels || channel >= self.channels.len() {
            return Err("only a derived channel can be removed".to_string());
        }
        let index = channel - self.base_channels;
        if self.derived.iter().enumerate().any(|(position, entry)| {
            position > index
                && (entry.spec().source() == channel
                    || entry.spec().ratio_denominator() == Some(channel))
        }) {
            return Err("a derived channel reads this derived channel".to_string());
        }
        if self
            .objects
            .iter()
            .any(|set| set.channel() >= channel || set.measured_channels() > channel)
        {
            return Err("an object set measures this derived channel".to_string());
        }
        if self.drifts.get(channel).is_some() {
            return Err("the drift correction aligns this derived channel".to_string());
        }
        self.derived.remove(index);
        self.channels.remove(channel);
        self.rebase_derived(channel);
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
        self.clear_object_coloc_from(channel);
        if self.selected >= channel {
            self.selected = self.selected.saturating_sub(1);
        }
        Ok(())
    }

    fn rebase_derived(&mut self, removed: usize) {
        for entry in &mut self.derived {
            let source = entry.spec().source();
            let denominator = entry.spec().ratio_denominator();
            let next_source = if source > removed { source - 1 } else { source };
            let next_denominator =
                denominator.map(|value| if value > removed { value - 1 } else { value });
            if next_source != source || next_denominator != denominator {
                entry.rebase(next_source, next_denominator);
            }
        }
    }

    pub fn set_restore(
        &mut self,
        channel: usize,
        psf: Psf,
        iterations: u32,
    ) -> Result<bool, String> {
        let operation = Operation::Restore(RestoreSpec::new(psf, iterations));
        self.replace_operation(channel, operation, None, |operation| {
            operation.restore().is_some()
        })
    }

    pub fn set_smooth(&mut self, channel: usize, sigma: f32) -> Result<bool, String> {
        let operation = Operation::Smooth(crate::derived::SmoothSpec::new(sigma));
        self.replace_operation(channel, operation, None, |operation| {
            operation.smooth().is_some()
        })
    }

    pub fn set_background(&mut self, channel: usize, radius: f32) -> Result<bool, String> {
        let operation = Operation::Background(crate::derived::BackgroundSpec::new(radius));
        self.replace_operation(channel, operation, None, |operation| {
            operation.background().is_some()
        })
    }

    pub fn set_ratio(
        &mut self,
        channel: usize,
        denominator: usize,
        intensity: Intensity,
    ) -> Result<bool, String> {
        if denominator >= self.channels.len()
            || denominator >= channel
            || denominator == self.derived_source(channel)?
        {
            return Err("a ratio channel divides two distinct channels of the dataset".to_string());
        }
        let operation = Operation::Ratio(crate::derived::RatioSpec::new(denominator));
        self.replace_operation(channel, operation, Some(intensity), |operation| {
            operation.ratio().is_some()
        })
    }

    fn replace_operation(
        &mut self,
        channel: usize,
        operation: Operation,
        intensity: Option<Intensity>,
        accepts: impl Fn(&Operation) -> bool,
    ) -> Result<bool, String> {
        if channel < self.base_channels || channel >= self.channels.len() {
            return Err("only a derived channel is an operator".to_string());
        }
        let index = channel - self.base_channels;
        let Some(entry) = self.derived.get(index) else {
            return Err("the derived channel is missing from the scene".to_string());
        };
        if !accepts(entry.spec().operation()) {
            return Err(format!(
                "{} is not the kind of operator this change edits",
                entry.spec().name()
            ));
        }
        let intensity = intensity.unwrap_or_else(|| entry.spec().intensity());
        if intensity.span() <= 0.0 {
            return Err("an operator channel declares a positive intensity span".to_string());
        }
        let spec = entry.spec().replacing(operation, intensity);
        let changed = spec != *entry.spec();
        if changed {
            self.derived[index] = Derived::plain(spec);
        }
        Ok(changed)
    }

    fn derived_source(&self, channel: usize) -> Result<usize, String> {
        if channel < self.base_channels || channel >= self.channels.len() {
            return Err("only a derived channel is an operator".to_string());
        }
        self.derived
            .get(channel - self.base_channels)
            .map(|entry| entry.spec().source())
            .ok_or_else(|| "the derived channel is missing from the scene".to_string())
    }

    pub fn sync_masks(&mut self) {
        for entry in &self.derived {
            let (Some(set), Some(channel)) = (entry.spec().mask(), entry.mask()) else {
                continue;
            };
            let Some(object) = self.objects.get(set) else {
                continue;
            };
            let stamp = self.gate_stamp(object);
            if channel.stamp() == stamp {
                continue;
            }
            if let Ok(gate) = self.mask_gate(object) {
                channel.refresh(gate, self.mask_radius, stamp);
            }
        }
    }

    pub fn mask_stamps(&self) -> Vec<u64> {
        self.derived
            .iter()
            .map(|entry| entry.mask().map_or(0, |mask| mask.stamp()))
            .collect()
    }

    pub fn damaged_channels(&self, roots: ChannelSet) -> ChannelSet {
        let mut channels = roots;
        for (index, entry) in self.derived.iter().enumerate() {
            let spec = entry.spec();
            let reads = channels.contains(spec.source())
                || spec
                    .ratio_denominator()
                    .is_some_and(|denominator| channels.contains(denominator));
            if reads {
                channels = channels.with(self.base_channels + index);
            }
        }
        channels
    }

    pub fn mask_extents(&self, time: u32) -> Vec<Option<MaskExtent>> {
        self.derived
            .iter()
            .map(|entry| {
                let set = entry.spec().mask()?;
                let frame = self.objects.get(set)?.frame_shared(time)?;
                frame.shape().ok()?.bounds()
            })
            .collect()
    }

    pub fn mask_damage(
        &self,
        channels: ChannelSet,
        extents: &[Option<MaskExtent>],
        previous: &[Option<MaskExtent>],
    ) -> Option<Region> {
        let voxel = self.dataset.voxel_size().max_element();
        let mut margin = self.mask_radius + voxel;
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        let mut bounded = false;
        for channel in channels.iter() {
            let Some(index) = channel.checked_sub(self.base_channels) else {
                continue;
            };
            let Some(entry) = self.derived.get(index) else {
                continue;
            };
            margin += entry.spec().operation().spread()?;
            let held = extents
                .get(index)
                .copied()
                .flatten()
                .into_iter()
                .chain(previous.get(index).copied().flatten());
            for (near, far) in held {
                low = low.min(Vec3::from(near));
                high = high.max(Vec3::from(far));
                bounded = true;
            }
        }
        if !bounded {
            return None;
        }
        let margin = Vec3::splat(margin);
        Some(Region::axis_aligned(low - margin, high + margin))
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
        if self.channel(index).color_table().is_some() {
            self.channels[index].window = self.channel(index).display();
            return;
        }
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
        let window = self.channel(index).display();
        self.channels[index].window = window;
    }

    pub fn set_transfer(&mut self, index: usize, transfer: TransferFunction) {
        self.channels[index].set_transfer(transfer);
    }

    pub fn set_volume_mode(&mut self, index: usize, mode: VolumeMode) {
        let channel = &mut self.channels[index];
        channel.set_volume(channel.volume().with_mode(mode));
    }

    pub fn set_isovalue(&mut self, index: usize, isovalue: f32) {
        let channel = &mut self.channels[index];
        channel.set_volume(channel.volume().with_isovalue(isovalue));
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

    pub fn import_graph(&mut self, file: &str, kind: ObjectKind) -> ObjectSetId {
        self.objects
            .import_graph(file, kind, self.selected, &self.dataset)
    }

    pub fn import_localizations(&mut self, file: &str) -> ObjectSetId {
        self.objects
            .import_localizations(file, self.selected, &self.dataset)
    }

    pub fn object_state(&self, id: ObjectSetId) -> Option<SetState> {
        self.objects.set_state(id)
    }

    pub fn restore_object(&mut self, state: SetState) -> bool {
        if !self.objects.restore_set(state, &self.dataset) {
            return false;
        }
        self.prune_mask_filter();
        self.sync_masks();
        self.sync_coloc_mask();
        self.validate_selections();
        true
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
        self.sync_coloc_mask();
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
        self.sync_coloc_mask();
    }

    pub fn reorder_object(&mut self, id: ObjectSetId, index: usize) -> bool {
        self.objects.reorder(id, index)
    }

    pub fn delete_object(
        &mut self,
        id: ObjectSetId,
        time: u32,
        object: ObjectId,
    ) -> Result<bool, String> {
        self.delete_objects(id, &[(time, vec![object])])
    }

    pub fn delete_objects(
        &mut self,
        id: ObjectSetId,
        plan: &[(u32, Vec<ObjectId>)],
    ) -> Result<bool, String> {
        let mut keeps = Vec::with_capacity(plan.len());
        for (time, objects) in plan {
            if objects.is_empty() {
                continue;
            }
            let frame = self
                .objects
                .get(id)
                .and_then(|set| set.frame_shared(*time))
                .ok_or_else(|| {
                    format!("the deleted objects name the frame {time} of no picture")
                })?;
            let mut keep = vec![true; frame.object_count()];
            for object in objects {
                let index = frame.index_of(*object).ok_or_else(|| {
                    format!(
                        "the frame {time} holds no object {} of the set",
                        object.raw()
                    )
                })?;
                keep[index] = false;
            }
            keeps.push((*time, keep));
        }
        if self.objects.retain_objects(id, &keeps)? == 0 {
            return Ok(false);
        }
        self.settle_install(id);
        self.sync_coloc_mask();
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
                Provenance::Filtered { source, .. }
                | Provenance::Edited { source, .. }
                | Provenance::Masked { source, .. } => {
                    current = *source;
                }
                Provenance::Detection
                | Provenance::Segmented { .. }
                | Provenance::Imported { .. }
                | Provenance::Interop { .. }
                | Provenance::Attached { .. }
                | Provenance::Copied { .. }
                | Provenance::Merged { .. } => {
                    return Some(current);
                }
            }
        }
    }

    pub fn dependents(&self, id: ObjectSetId) -> Vec<ObjectSetId> {
        self.objects.dependents(id)
    }

    pub fn masked_object(
        &mut self,
        source: ObjectSetId,
        suffix: &str,
        strokes: Vec<MaskEdit>,
    ) -> Option<ObjectSetId> {
        let masked = self.objects.masked(source, suffix, strokes)?;
        self.selections.clear();
        self.sync_coloc_mask();
        self.sync_masks();
        Some(masked)
    }

    pub fn replan_masked(&mut self, id: ObjectSetId, strokes: Vec<MaskEdit>) -> bool {
        self.objects.replan_masked(id, strokes)
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
        let ids = filtered.id_frames();
        let params = filtered.params();
        if ids.iter().all(ObjectIds::is_empty) {
            for time in 0..self.frame_count() {
                self.install_optional(id, time, None);
            }
            if let Some(set) = self.objects.get_mut(id) {
                set.set_params(params);
            }
            return Ok(true);
        }
        let tracks = filtered.tracks().map(|layer| layer.set());
        let groups = if criteria.group() == Group::Molecule {
            (0..self.frame_count())
                .map(|time| {
                    filtered.frame_shared(time).map_or_else(Vec::new, |frame| {
                        frame.spots_of().iter().map(|spot| spot.group()).collect()
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let table = self
            .table(source, criteria.group(), Aggregate::MEAN)
            .ok_or_else(|| {
                format!(
                    "{} holds no objects to filter into {}",
                    filtered.name(),
                    set.name()
                )
            })?;
        let keep = criteria.keep(&table, tracks, &groups, &ids)?;
        if keep.iter().flatten().all(|kept| !kept) {
            return Err(format!(
                "the filter of {} keeps no object of {}",
                set.name(),
                filtered.name()
            ));
        }
        for time in 0..self.frame_count() {
            let frame = self
                .objects
                .get(source)
                .and_then(|set| set.frame_shared(time));
            let pruned = match frame {
                Some(frame) => frame
                    .retain(&keep[time as usize])
                    .map_err(|error| error.to_string())?,
                None => None,
            };
            self.install_optional(id, time, pruned);
        }
        if let Some(set) = self.objects.get_mut(id) {
            set.set_params(params);
        }
        self.apply_curation(id)?;
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
            .any(|entry| entry.spec().mask() == Some(source))
        {
            return Err("a mask channel derives from the merged object set".to_string());
        }
        self.objects.merge(source, target)?;
        if self.mask == Some(source) {
            self.mask = None;
        }
        self.selections.retain(|selection| selection.set != source);
        self.prune_mask_filter();
        self.sync_coloc_mask();
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
        self.sync_coloc_mask();
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
        self.sync_coloc_mask();
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
        self.sync_coloc_mask();
        true
    }

    pub fn mask_filter_held(&self) -> Option<MaskFilter> {
        let filter = self.mask_filter?;
        let set = self.objects.get(self.mask?)?;
        (self.objects.get(filter.target()).is_some()
            && set.relation(filter.target(), filter.shift()).is_some())
        .then_some(filter)
    }

    pub(crate) fn prune_mask_filter(&mut self) {
        self.mask_filter = self.mask_filter_held();
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
        self.sync_coloc_mask();
        true
    }

    pub fn set_object_hidden(
        &mut self,
        id: ObjectSetId,
        time: u32,
        ids: &[ObjectId],
        hidden: bool,
    ) -> bool {
        self.objects.set_hidden(id, time, ids, hidden)
    }

    pub fn clear_hidden(&mut self, id: ObjectSetId) -> bool {
        self.objects.clear_hidden(id)
    }

    pub fn hidden_count(&self, id: ObjectSetId) -> usize {
        self.objects.get(id).map_or(0, ObjectSet::hidden_count)
    }

    pub fn set_label_metric(&mut self, id: ObjectSetId, metric: Option<ObjectColor>) -> bool {
        self.objects.set_label_metric(id, metric)
    }

    pub fn set_object_material(&mut self, id: ObjectSetId, material: Material) -> bool {
        self.objects.get_mut(id).is_some_and(|set| {
            let previous = set.material();
            set.set_material(material);
            set.material() != previous
        })
    }

    pub fn set_object_coloring(
        &mut self,
        id: ObjectSetId,
        coloring: Coloring<ObjectColor>,
    ) -> bool {
        self.objects.set_object_coloring(id, coloring)
    }

    pub fn set_label_export(&mut self, id: ObjectSetId, export: Option<LabelExport>) -> bool {
        self.objects
            .get_mut(id)
            .is_some_and(|set| set.set_label_export(export))
    }

    pub fn object_colors(&self, set: &ObjectSet) -> Option<Vec<[f32; 3]>> {
        let metric = set.object_coloring().metric()?;
        if metric == ObjectColor::Identity {
            let frame = set.frame_shared(self.time)?;
            return Some((0..frame.object_count()).map(categorical).collect());
        }
        let extent = self.objects.object_extent(set.id())?;
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

    pub fn object_extent(&self, set: &ObjectSet) -> Option<Extent> {
        self.objects.object_extent(set.id())
    }

    pub fn label_values(&self, set: &ObjectSet) -> Option<Arc<Vec<f32>>> {
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
        let frame = self.objects.get(child)?.frame_shared(self.time)?;
        let parents = self.objects.get(parent)?.frame_shared(self.time)?;
        let query = self.objects.child(parent, child, self.time, &frame)?;
        let values = query.values(&parents.objects(), grana_measure::ChildField::Count);
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
        self.coloc.clear();
        self.clear_object_coloc_all();
    }

    pub fn region_scope(&self) -> Option<Region> {
        self.region.enabled().then_some(self.region)
    }

    pub fn measure_bounds(&self) -> (Vec3, Vec3) {
        match self.region_scope() {
            Some(region) => region.bounds(),
            None => {
                let half = 0.5 * self.dataset.world_size();
                (-half, half)
            }
        }
    }

    pub fn set_clip(&mut self, clip: Clip) {
        self.clip = clip;
    }

    pub fn recalibrate(&mut self, spacing: Spacing) {
        let calibration = self.dataset.pyramid().calibration();
        let calibration = match calibration.measured() {
            true => calibration,
            false => Calibration::Declared,
        };
        self.place_spacing(spacing, calibration);
    }

    fn place_spacing(&mut self, spacing: Spacing, calibration: Calibration) {
        if self.dataset.spacing() == spacing && self.dataset.pyramid().calibration() == calibration
        {
            return;
        }
        self.dataset = self.dataset.with_calibration(spacing, calibration);
        let half = 0.5 * self.dataset.world_size();
        self.region = Region::full(-half, half);
        self.clip = Clip::none();
        self.annotations = Annotations::new();
        self.set_drifts(Drifts::new());
        self.set_registration(Registration::default());
        self.mask = None;
        self.mask_filter = None;
        self.mask_radius = 0.0;
        self.objects = Objects::new();
        self.selections.clear();
        self.set_derived_layout(Vec::new())
            .expect("the base channels fit the channel limit");
        self.coloc = Coloc::new(self.channels.len());
    }

    pub fn drifts(&self) -> &Drifts {
        &self.drifts
    }

    pub fn drift(&self, channel: usize) -> Option<&Drift> {
        self.drifts.get(channel)
    }

    pub fn drift_warp(&self, channel: usize, time: u32) -> &grana_warp::Warp {
        self.drifts.warp(channel, time)
    }

    pub fn drift_warps(&self, time: u32) -> grana_warp::Warps {
        self.drifts.warps(time, self.base_channels)
    }

    pub fn set_drift(&mut self, drift: Drift) -> bool {
        assert!(
            drift.channel() < self.base_channels,
            "a drift correction aligns a stored channel"
        );
        if self.drifts.get(drift.channel()) == Some(&drift) {
            return false;
        }
        self.drifts = self.drifts.placed(drift);
        self.realigned();
        true
    }

    pub fn clear_drift(&mut self, channel: usize) -> bool {
        if self.drifts.get(channel).is_none() {
            return false;
        }
        self.drifts = self.drifts.without(channel);
        self.realigned();
        true
    }

    pub fn set_drifts(&mut self, drifts: Drifts) -> bool {
        if self.drifts == drifts {
            return false;
        }
        self.drifts = drifts.fitted(self.base_channels);
        self.realigned();
        true
    }

    fn realigned(&mut self) {
        self.objects
            .realign_localizations(&self.drifts, &self.registration);
        self.coloc.clear();
        self.clear_object_coloc_all();
    }

    pub fn set_localization(
        &mut self,
        id: ObjectSetId,
        source: crate::localization::LocalizationSource,
    ) -> bool {
        self.objects.set_localization(id, source)
    }

    pub fn set_localization_spec(
        &mut self,
        id: ObjectSetId,
        spec: grana_object::LocalizationSpec,
    ) -> bool {
        self.objects.set_localization_spec(id, spec)
    }

    pub fn set_localization_group_colored(&mut self, id: ObjectSetId, group_colored: bool) -> bool {
        self.objects
            .set_localization_group_colored(id, group_colored)
    }

    pub fn set_localization_projected(&mut self, id: ObjectSetId, projected: bool) -> bool {
        self.objects.set_localization_projected(id, projected)
    }

    pub fn set_localization_clustered(&mut self, id: ObjectSetId, clustered: bool) -> bool {
        self.objects.set_localization_clustered(id, clustered)
    }

    pub fn set_localization_clusters(
        &mut self,
        id: ObjectSetId,
        clusters: grana_object::ClusterSpec,
    ) -> bool {
        self.objects.set_localization_clusters(id, clusters)
    }

    pub fn regroup(&mut self, id: ObjectSetId, groups: &[Vec<Option<u32>>]) -> bool {
        self.objects.regroup(id, groups)
    }

    pub fn install_molecules(
        &mut self,
        id: ObjectSetId,
        groups: &[Vec<Option<u32>>],
    ) -> Result<bool, String> {
        self.objects.set_localization_clustered(id, true);
        if !self.regroup(id, groups) {
            return Ok(false);
        }
        self.rebuild_molecule_tracks(id)
    }

    pub fn rebuild_molecule_tracks(&mut self, id: ObjectSetId) -> Result<bool, String> {
        let Some(object) = self.objects.get(id) else {
            return Ok(false);
        };
        let mut grouped = Vec::new();
        for (time, frame) in object.frames() {
            let Some(spots) = frame.spots() else {
                continue;
            };
            for (index, spot) in spots.spots().iter().enumerate() {
                let Some(group) = spot.group() else {
                    continue;
                };
                if object.visible_object(time, index)
                    && let Some(id) = spots.id(index)
                {
                    grouped.push((group, time, id, spot.center()));
                }
            }
        }
        if grouped.is_empty() {
            return Ok(false);
        }
        let tracks = grana_object::grouped_tracks(self.dataset.frame_interval(), grouped)?;
        self.install_tracks(id, tracks);
        Ok(true)
    }

    pub fn realign_localization(&mut self, id: ObjectSetId) {
        let Some(channel) = self.objects.get(id).map(|set| set.channel()) else {
            return;
        };
        let next = LocalizationAlignment::new(
            channel,
            self.drifts.get(channel).cloned().unwrap_or_default(),
            self.registration.clone(),
        );
        self.objects.realign_localization(id, &next);
    }

    pub fn registration(&self) -> &Registration {
        &self.registration
    }

    pub fn set_registration(&mut self, registration: Registration) -> bool {
        if self.registration == registration {
            return false;
        }
        self.registration = registration;
        self.objects
            .realign_localizations(&self.drifts, &self.registration);
        self.coloc.clear();
        self.clear_object_coloc_all();
        true
    }

    pub fn mask_frames(&self) -> Result<Option<grana_object::Roi>, String> {
        let Some(set) = self.mask.and_then(|mask| self.objects.get(mask)) else {
            return Ok(None);
        };
        let gate = self.mask_gate(set)?;
        Ok(Some(gate.roi().with_radius(self.mask_radius)))
    }

    fn mask_gate(&self, set: &ObjectSet) -> Result<Arc<MaskGate>, String> {
        let filter = self.mask_filter;
        let relation = match filter {
            None => None,
            Some(filter) => {
                let relation = set
                    .relation(filter.target(), filter.shift())
                    .ok_or_else(|| {
                        format!(
                            "the analysis mask relates {} to an object set that left the scene",
                            set.name()
                        )
                    })?;
                let target = self
                    .objects
                    .get(filter.target())
                    .map_or("the related set", ObjectSet::name);
                Some(RelationGate::new(
                    relation.shared_frames().to_vec(),
                    set.name().to_string(),
                    target.to_string(),
                ))
            }
        };
        Ok(MaskGate::new(set.source(), filter, relation))
    }

    fn gate_stamp(&self, set: &ObjectSet) -> u64 {
        let mut stamp = mix(HASH_OFFSET, set.shape().raw());
        stamp = mix(stamp, u64::from(self.mask_radius.to_bits()));
        stamp = mix(stamp, filter_stamp(self.mask_filter));
        stamp
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
            self.sync_coloc_mask();
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
        self.sync_coloc_mask();
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
        TableRequest::new(self, id, group, aggregate)?.build()
    }

    pub fn train_classifier(
        &mut self,
        id: ObjectSetId,
        features: &[Measurement],
        positives: &[u32],
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
            .table(id, Group::Object, Aggregate::MEAN)
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

    pub fn install_object_coloc(&mut self, id: ObjectSetId, coloc: ObjectColoc) -> bool {
        self.objects.set_object_coloc(id, Some(coloc))
    }

    pub fn clear_object_coloc(&mut self, id: ObjectSetId) -> bool {
        self.objects.set_object_coloc(id, None)
    }

    pub fn begin_replace(&mut self, id: ObjectSetId) -> bool {
        self.objects
            .get_mut(id)
            .is_some_and(ObjectSet::begin_replace)
    }

    pub fn end_replace(&mut self, id: ObjectSetId) {
        if self.objects.get_mut(id).is_some_and(ObjectSet::end_replace) {
            self.settle_install(id);
            self.sync_masks();
        }
    }

    pub fn install(
        &mut self,
        id: ObjectSetId,
        time: u32,
        frame: ObjectFrame,
    ) -> Option<Readoption> {
        let report = self.objects.install(id, time, frame)?;
        if !self.replacing(id) {
            self.settle_install(id);
        }
        Some(report)
    }

    fn replacing(&self, id: ObjectSetId) -> bool {
        self.objects.get(id).is_some_and(ObjectSet::replacing)
    }

    pub fn install_shared(
        &mut self,
        id: ObjectSetId,
        time: u32,
        frame: Arc<ObjectFrame>,
    ) -> Option<Readoption> {
        let report = self.objects.install_shared(id, time, frame)?;
        if !self.replacing(id) {
            self.settle_install(id);
        }
        Some(report)
    }

    pub fn install_optional(
        &mut self,
        id: ObjectSetId,
        time: u32,
        frame: Option<ObjectFrame>,
    ) -> Option<Readoption> {
        let report = self
            .objects
            .install_optional(id, time, frame.map(Arc::new))?;
        if !self.replacing(id) {
            self.settle_install(id);
        }
        Some(report)
    }

    fn settle_install(&mut self, id: ObjectSetId) {
        let held = self
            .objects
            .get(id)
            .map(|set| (0..set.frame_count()).map(|time| set.frame_shared(time)))
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        self.selections.retain(|selection| {
            if selection.set != id {
                return true;
            }
            let Item::Object(id) = selection.item else {
                return true;
            };
            held.iter().any(|frame| {
                frame
                    .as_ref()
                    .is_some_and(|frame| frame.index_of(id).is_some())
            })
        });
        self.retain_track_selection(id);
        self.sync_coloc_mask();
        self.sync_masks();
    }

    pub fn clear_object(&mut self, id: ObjectSetId) {
        self.objects.clear(id);
        self.objects.clear_relations(id);
        self.selections.retain(|selection| selection.set != id);
        self.prune_mask_filter();
        self.sync_coloc_mask();
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        if self.objects.install_tracks(id, set) {
            self.retain_track_selection(id);
        }
    }

    pub fn apply_curation(&mut self, id: ObjectSetId) -> Result<usize, String> {
        let applied = self.objects.apply_curation(id)?;
        self.retain_track_selection(id);
        self.sync_masks();
        Ok(applied)
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
            .any(|entry| entry.spec().mask() == Some(set))
        {
            return;
        }
        let mut specs = self.derived_specs();
        while let Some(index) = specs.iter().position(|spec| spec.mask() == Some(set)) {
            let removed = self.base_channels + index;
            specs.remove(index);
            let mut surviving = Vec::with_capacity(specs.len());
            for spec in specs {
                let source = match spec.source().cmp(&removed) {
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some(spec.source() - 1),
                    std::cmp::Ordering::Less => Some(spec.source()),
                };
                let had_denominator = spec.ratio_denominator().is_some();
                let denominator =
                    spec.ratio_denominator().and_then(|denominator| {
                        match denominator.cmp(&removed) {
                            std::cmp::Ordering::Equal => None,
                            std::cmp::Ordering::Greater => Some(denominator - 1),
                            std::cmp::Ordering::Less => Some(denominator),
                        }
                    });
                let Some(source) = source else {
                    continue;
                };
                if had_denominator && denominator.is_none() {
                    continue;
                }
                surviving.push(spec.rebased(source, denominator));
            }
            specs = surviving;
        }
        let infos =
            crate::derived::derived_infos(&specs, &self.dataset.channels()[..self.base_channels])
                .expect("the surviving derived channels read channels the scene holds");
        self.set_derived_layout(infos)
            .expect("removing mask channels leaves room for the survivors");
        self.bind_derived(specs)
            .expect("the surviving derived channels keep their names");
        self.sync_masks();
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        self.objects.clear_tracks(id);
        self.retain_track_selection(id);
    }

    pub fn cut_track(&mut self, id: ObjectSetId, time: u32, object: ObjectId) -> bool {
        if !self.objects.cut_track(id, time, object) {
            return false;
        }
        self.retain_track_selection(id);
        true
    }

    pub fn join_tracks(
        &mut self,
        id: ObjectSetId,
        source: (u32, ObjectId),
        target: (u32, ObjectId),
    ) -> bool {
        if !self.objects.join_tracks(id, source, target) {
            return false;
        }
        self.retain_track_selection(id);
        true
    }

    pub fn divide_tracks(
        &mut self,
        id: ObjectSetId,
        parent: (u32, ObjectId),
        first: (u32, ObjectId),
        second: (u32, ObjectId),
    ) -> bool {
        if !self.objects.divide_tracks(id, parent, first, second) {
            return false;
        }
        self.retain_track_selection(id);
        true
    }

    pub fn merge_tracks(
        &mut self,
        id: ObjectSetId,
        first: (u32, ObjectId),
        second: (u32, ObjectId),
        child: (u32, ObjectId),
    ) -> bool {
        if !self.objects.merge_tracks(id, first, second, child) {
            return false;
        }
        self.retain_track_selection(id);
        true
    }

    pub fn reparent_track(
        &mut self,
        id: ObjectSetId,
        child: (u32, ObjectId),
        parent: (u32, ObjectId),
    ) -> bool {
        if !self.objects.reparent_track(id, child, parent) {
            return false;
        }
        self.retain_track_selection(id);
        true
    }

    pub fn detach_track(&mut self, id: ObjectSetId, child: (u32, ObjectId)) -> bool {
        if !self.objects.detach_track(id, child) {
            return false;
        }
        self.retain_track_selection(id);
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
                .and_then(|set| set.frame_shared(self.time))
                .map_or(0, |frame| frame.object_count()),
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
                    .frame_shared(time)
                    .map_or_else(Vec::new, |frame| frame.centroids())
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
        let Some(frame) = self
            .objects
            .get(id)
            .and_then(|set| set.frame_shared(self.time))
        else {
            return Ok(None);
        };
        let mesh = frame.geometry().map_err(|error| error.to_string())?;
        Ok(mesh.and_then(|mesh| ray_hit(&mesh, origin, direction)))
    }

    pub fn surface_hit(&self, origin: Vec3, direction: Vec3) -> Result<Option<Vec3>, String> {
        let mut nearest: Option<Vec3> = None;
        for set in self.pickable() {
            let Some(frame) = set.frame_shared(self.time) else {
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

    fn retain_track_selection(&mut self, id: ObjectSetId) {
        let held = self
            .objects
            .get(id)
            .and_then(ObjectSet::tracks)
            .map(TrackLayer::set);
        self.selections.retain(|selection| {
            if !matches!(selection.item, Item::Track(_)) {
                return true;
            }
            if selection.set != id {
                return true;
            }
            match selection.item {
                Item::Track(track) => held.is_some_and(|set| set.contains(track)),
                Item::Object(_) => true,
            }
        });
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

    pub fn select_many(&mut self, selections: Vec<Selection>) {
        self.selections = selections;
    }

    pub fn toggle_select(&mut self, selection: Selection) {
        match self.selections.iter().position(|held| *held == selection) {
            Some(position) => {
                self.selections.remove(position);
            }
            None => self.selections.push(selection),
        }
    }

    pub fn select_track(&mut self, id: ObjectSetId, track: TrackId) {
        assert!(
            self.objects
                .get(id)
                .and_then(ObjectSet::tracks)
                .is_some_and(|layer| layer.set().contains(track)),
            "the selected track exists"
        );
        self.select(Some(Selection {
            set: id,
            item: Item::Track(track),
        }));
    }

    pub fn selected_track(&self, id: ObjectSetId) -> Option<TrackId> {
        match self.selection() {
            Some(Selection {
                set,
                item: Item::Track(track),
            }) if set == id => Some(track),
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
        let Some(id) = self
            .objects
            .get(set)
            .and_then(|set| set.frame_shared(self.time))
            .and_then(|frame| frame.id_of(index))
        else {
            return false;
        };
        self.selections.contains(&Selection {
            set,
            item: Item::Object(id),
        })
    }

    pub fn track_selected(&self, set: ObjectSetId, track: TrackId) -> bool {
        self.selections.contains(&Selection {
            set,
            item: Item::Track(track),
        })
    }

    pub fn mask_key(&self) -> Option<MaskKey> {
        let set = self.objects.get(self.mask?)?;
        Some(MaskKey::new(
            set.id(),
            set.shape().raw(),
            self.mask_radius,
            self.mask_filter,
        ))
    }

    fn sync_coloc_mask(&mut self) {
        let mask = self.mask_key();
        self.coloc.set_mask(mask);
    }

    fn clear_object_coloc_all(&mut self) {
        let stale = self
            .objects
            .iter()
            .filter(|set| set.object_coloc().is_some())
            .map(|set| set.id())
            .collect::<Vec<_>>();
        for id in stale {
            self.objects.set_object_coloc(id, None);
        }
    }
}

fn filter_stamp(filter: Option<MaskFilter>) -> u64 {
    let Some(filter) = filter else {
        return 0;
    };
    let mut stamp = filter.target().raw();
    stamp = mix(stamp, filter.shift().cast_unsigned() as u64);
    stamp = mix(stamp, u64::from(filter.field().index()));
    stamp = mix(stamp, u64::from(filter.range()[0].to_bits()));
    mix(stamp, u64::from(filter.range()[1].to_bits()))
}

pub(crate) fn initial_transfer(info: &ChannelInfo) -> TransferFunction {
    match info.color_table() {
        Some(color_table) => TransferFunction::indexed(color_table.colors()),
        None => TransferFunction::tinted(info.tint()),
    }
}
