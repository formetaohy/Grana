use std::collections::HashSet;

use grana_measure::{ChildField, ChildMetric, Measurement, RelationField};
use grana_volume::CHANNEL_LIMIT;

use crate::object_set::ObjectSetId;
use crate::state::{ItemState, SceneState, SetState};
use crate::{ObjectColor, ObjectKind, Seeds, TrackColor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneShape {
    base_channels: usize,
    frame_count: u32,
}

impl SceneShape {
    pub fn new(base_channels: usize, frame_count: u32) -> Self {
        assert!(
            base_channels > 0 && frame_count > 0,
            "a dataset holds a channel and a frame"
        );
        Self {
            base_channels,
            frame_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneFault(&'static str);

impl SceneFault {
    pub fn detail(self) -> &'static str {
        self.0
    }
}

impl SceneState {
    pub fn check(&self) -> Result<(), SceneFault> {
        self.channels()?;
        self.annotations()?;
        self.sets()?;
        self.coloc()?;
        self.mask_filter()?;
        self.selections()?;
        Ok(())
    }

    pub fn validate(&self, shape: SceneShape) -> Result<(), SceneFault> {
        self.check()?;
        if self.channels.len() != shape.base_channels + self.derived.len() {
            return Err(fault(
                "the scene state carries another channel layout than its dataset",
            ));
        }
        if self.time >= shape.frame_count {
            return Err(fault(
                "the scene state names a frame the dataset does not hold",
            ));
        }
        let frames = shape.frame_count as usize;
        if self.sets.iter().any(|set| {
            set.frames.len() != frames
                || set
                    .relations
                    .iter()
                    .any(|relation| relation.frames.len() != frames)
        }) {
            return Err(fault(
                "the scene state carries another frame count than its dataset",
            ));
        }
        if self.drifts.held().any(|drift| {
            drift.frames() != shape.frame_count
                || drift.reference() >= shape.frame_count
                || drift.channel() >= shape.base_channels
        }) || self.drifts.channels() > shape.base_channels
        {
            return Err(fault(
                "the drift correction covers another time series than its dataset",
            ));
        }
        if !self.registration.is_empty()
            && (self.registration.channels() != shape.base_channels
                || self.registration.reference() >= self.registration.channels())
        {
            return Err(fault(
                "the registration aligns another channel layout than its dataset",
            ));
        }
        if self
            .coloc
            .results
            .iter()
            .any(|result| result.series().frames().len() != frames)
        {
            return Err(fault(
                "the colocalization result covers another time series than its dataset",
            ));
        }
        if self.annotations.entries.iter().any(|entry| {
            entry
                .frame()
                .is_some_and(|frame| frame >= shape.frame_count)
        }) {
            return Err(fault(
                "an annotation names a frame the dataset does not hold",
            ));
        }
        Ok(())
    }

    fn channels(&self) -> Result<(), SceneFault> {
        let channels = self.channels.len();
        if channels == 0 || channels > CHANNEL_LIMIT {
            return Err(fault("the scene state carries an impossible channel count"));
        }
        let Some(base) = channels.checked_sub(self.derived.len()) else {
            return Err(fault(
                "the scene state carries more derived channels than channels",
            ));
        };
        for (index, spec) in self.derived.iter().enumerate() {
            let channel = base + index;
            if spec.source() >= channel {
                return Err(fault(
                    "a derived channel reads itself or a channel that follows it",
                ));
            }
            if spec
                .ratio_denominator()
                .is_some_and(|denominator| denominator >= channel || denominator == spec.source())
            {
                return Err(fault(
                    "a ratio channel divides itself or a channel that follows it",
                ));
            }
            if spec
                .mask()
                .is_some_and(|set| !self.sets.iter().any(|candidate| candidate.id == set.raw()))
            {
                return Err(fault("a mask channel names an object set of the scene"));
            }
            if spec.name().trim().is_empty() {
                return Err(fault("a derived channel carries a name of its own"));
            }
            if self.derived[..index]
                .iter()
                .any(|other| other.name() == spec.name())
            {
                return Err(fault("the scene state names a derived channel twice"));
            }
        }
        if self.selected >= channels {
            return Err(fault(
                "the scene state selects a channel the dataset does not hold",
            ));
        }
        if !(self.mask_radius.is_finite() && self.mask_radius >= 0.0) {
            return Err(fault("the analysis mask grows by a finite radius"));
        }
        Ok(())
    }

    fn annotations(&self) -> Result<(), SceneFault> {
        if self
            .annotations
            .selected
            .is_some_and(|index| index >= self.annotations.entries.len())
        {
            return Err(fault(
                "the scene state selects an annotation it does not hold",
            ));
        }
        Ok(())
    }

    fn sets(&self) -> Result<(), SceneFault> {
        let channels = self.channels.len();
        let mut ids = HashSet::with_capacity(self.sets.len());
        let mut keys = HashSet::with_capacity(self.sets.len());
        let frames = self.sets.first().map(|set| set.frames.len());
        for set in &self.sets {
            if set.id == 0 {
                return Err(fault("an object set carries an impossible identity"));
            }
            if !ids.insert(set.id) {
                return Err(fault("the scene state names an object set twice"));
            }
            if !keys.insert(set.key.as_str()) {
                return Err(fault("the scene state names an object set key twice"));
            }
            if set.name.trim().is_empty() {
                return Err(fault("an object set carries a name of its own"));
            }
            if set.channel >= channels {
                return Err(fault(
                    "an object set detects on a channel the dataset does not hold",
                ));
            }
            if frames.is_some_and(|frames| set.frames.len() != frames) {
                return Err(fault("the object sets cover another set of frames"));
            }
            if set.document.is_zero() {
                return Err(fault("an object set carries an impossible document"));
            }
            if set.params.kind() != set.kind {
                return Err(fault("an object set detects another kind than it holds"));
            }
            self.set_contents(set)?;
            self.set_relations(set)?;
            self.set_seeds(set)?;
            self.set_metrics(set)?;
            self.set_provenance(set)?;
        }
        if !provenance_acyclic(&self.sets) {
            return Err(fault("the object sets derive from each other in a cycle"));
        }
        for set in &self.sets {
            if set
                .provenance
                .sources()
                .iter()
                .any(|source| source.raw() == set.id || !ids.contains(&source.raw()))
            {
                return Err(fault(
                    "an object set derives from a set the scene does not hold",
                ));
            }
        }
        Ok(())
    }

    fn set_contents(&self, set: &SetState) -> Result<(), SceneFault> {
        if set.kind.trackable() != set.track_style.is_some()
            || (set.tracks.is_some() && set.track_style.is_none())
            || (!set.kind.trackable() && set.track_color.metric.is_some())
        {
            return Err(fault(
                "an object set carries a track style that its kind does not track",
            ));
        }
        if set
            .frames
            .iter()
            .flatten()
            .any(|frame| frame.kind() != set.kind)
        {
            return Err(fault("an object set holds a frame of another kind"));
        }
        if !set.hidden.is_empty()
            && (set.hidden.len() != set.frames.len()
                || set
                    .hidden
                    .iter()
                    .zip(&set.frames)
                    .any(|(mask, frame)| mask.len() != object_count(frame)))
        {
            return Err(fault("the hidden objects cover another set of frames"));
        }
        if !set.curation.is_empty() && set.curation.frames() != set.frames.len() {
            return Err(fault("the manual edits cover another set of frames"));
        }
        if let Some(coloc) = &set.object_coloc
            && (coloc.pair().a() >= self.channels.len()
                || coloc.pair().b() >= self.channels.len()
                || coloc.frames().len() != set.frames.len()
                || coloc
                    .frames()
                    .iter()
                    .zip(&set.frames)
                    .any(|(values, frame)| values.len() != object_count(frame)))
        {
            return Err(fault(
                "the object colocalization covers another set of frames",
            ));
        }
        if let Some(localization) = &set.localization {
            let applied = localization.applied();
            if set.kind != ObjectKind::Spots
                || localization.spec() != localization.spec().fitted(set.frames.len().max(1) as u32)
                || !applied.is_empty()
                    && (applied.channel() != set.channel
                        || !applied.drift().is_empty()
                            && (applied.drift().channel() != set.channel
                                || applied.drift().frames() as usize != set.frames.len()
                                || applied.drift().reference() as usize >= set.frames.len()))
            {
                return Err(fault(
                    "the localization source covers another set of frames",
                ));
            }
            if localization.spec().filter().photons() > 0.0 && !localization.photons() {
                return Err(fault(
                    "the localization filter reads photons the table does not carry",
                ));
            }
        }
        Ok(())
    }

    fn set_relations(&self, set: &SetState) -> Result<(), SceneFault> {
        let frames = set.frames.len();
        let mut pairs = HashSet::with_capacity(set.relations.len());
        let mut keys = HashSet::with_capacity(set.relations.len());
        for relation in &set.relations {
            if !pairs.insert((relation.target, relation.shift))
                || !keys.insert(relation.key.as_str())
                || relation.frames.len() != frames
                || relation.shift.unsigned_abs() as usize >= frames
            {
                return Err(fault("the scene state carries an impossible relation"));
            }
            if relation
                .frames
                .iter()
                .zip(&set.frames)
                .any(|(frame, objects)| {
                    frame
                        .as_ref()
                        .is_some_and(|frame| frame.objects() != object_count(objects))
                })
            {
                return Err(fault(
                    "a relation covers another number of objects than its frame",
                ));
            }
        }
        Ok(())
    }

    fn set_seeds(&self, set: &SetState) -> Result<(), SceneFault> {
        match &set.seeds {
            Seeds::Points(frames) => {
                if !frames.is_empty() && frames.len() != set.frames.len() {
                    return Err(fault("the seeds cover another set of frames"));
                }
                if frames.iter().any(|frame| {
                    frame
                        .iter()
                        .any(|seed| seed.iter().any(|value| !value.is_finite()))
                }) {
                    return Err(fault("a seed names a finite position"));
                }
                if set.kind != ObjectKind::Surfaces && frames.iter().any(|frame| !frame.is_empty())
                {
                    return Err(fault("only a surface set carries seeds"));
                }
            }
            Seeds::Source(source) => {
                if !matches!(set.kind, ObjectKind::Surfaces | ObjectKind::Cells)
                    || source.raw() == set.id
                {
                    return Err(fault("a seed source names another surface or cell set"));
                }
                if !self
                    .sets
                    .iter()
                    .any(|candidate| candidate.id == source.raw())
                {
                    return Err(fault(
                        "a seed source names an object set the scene does not hold",
                    ));
                }
            }
        }
        Ok(())
    }

    fn set_metrics(&self, set: &SetState) -> Result<(), SceneFault> {
        let channels = self.channels.len();
        if let Some(classifier) = &set.classifier
            && classifier.features.iter().any(|feature| {
                !set.kind.measurements(channels).contains(feature)
                    || match feature {
                        Measurement::Intensity { channel, .. } => *channel >= channels,
                        _ => false,
                    }
            })
        {
            return Err(fault(
                "a classifier names a measurement its set does not carry",
            ));
        }
        if set
            .object_color
            .metric
            .is_some_and(|metric| !color_applies(&self.sets, set, channels, metric))
        {
            return Err(fault(
                "an object color names a measurement its set does not carry",
            ));
        }
        if set
            .label
            .is_some_and(|metric| !color_applies(&self.sets, set, channels, metric))
        {
            return Err(fault(
                "an object label names a measurement its set does not carry",
            ));
        }
        if set
            .track_color
            .metric
            .is_some_and(|metric| !track_color_applies(set, metric))
        {
            return Err(fault(
                "a track color names a measurement its set does not carry",
            ));
        }
        Ok(())
    }

    fn set_provenance(&self, set: &SetState) -> Result<(), SceneFault> {
        let channels = self.channels.len();
        if set
            .provenance
            .parts()
            .iter()
            .any(|part| part.channel() >= channels || part.kind() != set.kind)
        {
            return Err(fault(
                "an object set merges a part of another channel or kind",
            ));
        }
        for part in set.provenance.parts() {
            match part.seeds() {
                Seeds::Points(frames) => {
                    if frames.is_empty() {
                        continue;
                    }
                    if frames.len() != set.frames.len() {
                        return Err(fault("a merged part covers another set of frames"));
                    }
                }
                Seeds::Source(_) => {
                    if !matches!(part.kind(), ObjectKind::Surfaces | ObjectKind::Cells) {
                        return Err(fault("only a surface part carries a seed source"));
                    }
                }
            }
        }
        Ok(())
    }

    fn coloc(&self) -> Result<(), SceneFault> {
        let channels = self.channels.len();
        let frames = self.sets.first().map(|set| set.frames.len());
        if self
            .coloc
            .pair
            .is_some_and(|pair| pair.a() >= channels || pair.b() >= channels)
        {
            return Err(fault(
                "the colocalization pair names a channel the dataset does not hold",
            ));
        }
        if self.coloc.revision.is_zero() {
            return Err(fault(
                "the colocalization state carries an impossible document",
            ));
        }
        for (index, result) in self.coloc.results.iter().enumerate() {
            let pair = result.pair();
            if pair.a() >= channels
                || pair.b() >= channels
                || frames.is_some_and(|frames| result.series().frames().len() != frames)
                || result
                    .mask()
                    .is_some_and(|mask| !self.sets.iter().any(|set| set.id == mask))
                || self.coloc.results[index + 1..]
                    .iter()
                    .any(|other| other.pair() == pair)
            {
                return Err(fault(
                    "the colocalization result names a channel or mask the scene does not hold",
                ));
            }
        }
        Ok(())
    }

    fn mask_filter(&self) -> Result<(), SceneFault> {
        let Some(filter) = self.mask_filter else {
            return Ok(());
        };
        let Some(mask) = self
            .mask
            .and_then(|mask| self.sets.iter().find(|set| set.id == mask))
        else {
            return Err(fault(
                "an analysis filter names an analysis mask that is missing",
            ));
        };
        if !self.sets.iter().any(|set| set.id == filter.target().raw())
            || !mask.relations.iter().any(|relation| {
                relation.target == filter.target().raw() && relation.shift == filter.shift()
            })
        {
            return Err(fault(
                "an analysis filter names a relation the analysis mask does not measure",
            ));
        }
        Ok(())
    }

    fn selections(&self) -> Result<(), SceneFault> {
        for selection in &self.selections {
            let Some(set) = self.sets.iter().find(|set| set.id == selection.set) else {
                return Err(fault(
                    "the scene state selects an object set it does not hold",
                ));
            };
            let valid = match selection.item {
                ItemState::Object(index) => set
                    .frames
                    .iter()
                    .any(|frame| frame.as_ref().is_some_and(|frame| index < frame.objects())),
                ItemState::Track(index) => set
                    .tracks
                    .as_ref()
                    .is_some_and(|tracks| index < tracks.len()),
            };
            if !valid {
                return Err(fault(
                    "the scene state selects an object its set does not hold",
                ));
            }
        }
        Ok(())
    }
}

fn object_count(frame: &Option<crate::frame::Frame>) -> usize {
    frame.as_ref().map_or(0, crate::frame::Frame::objects)
}

fn provenance_acyclic(sets: &[SetState]) -> bool {
    let mut remaining = sets
        .iter()
        .map(|set| {
            (
                set.id,
                set.provenance
                    .sources()
                    .into_iter()
                    .map(ObjectSetId::raw)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    while !remaining.is_empty() {
        let removable = remaining
            .iter()
            .enumerate()
            .filter(|(_, (_, sources))| {
                sources
                    .iter()
                    .all(|source| !remaining.iter().any(|(id, _)| id == source))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if removable.is_empty() {
            return false;
        }
        for index in removable.into_iter().rev() {
            remaining.remove(index);
        }
    }
    true
}

fn color_applies(sets: &[SetState], set: &SetState, channels: usize, metric: ObjectColor) -> bool {
    match metric {
        ObjectColor::Identity => true,
        ObjectColor::Measurement(measurement) => {
            measurement.colorable() && set.kind.measurements(channels).contains(&measurement)
        }
        ObjectColor::Relation {
            target,
            field,
            shift,
        } => {
            RelationField::CONTINUOUS.contains(&field)
                && set
                    .relations
                    .iter()
                    .any(|relation| relation.target == target.raw() && relation.shift == shift)
        }
        ObjectColor::Score => set.classifier.is_some(),
        ObjectColor::Coloc { pair, .. } => pair.a() < channels && pair.b() < channels,
        ObjectColor::Child { target, field } => {
            let Some(child) = sets.iter().find(|candidate| candidate.id == target.raw()) else {
                return false;
            };
            let metrics = child
                .kind
                .measurements(channels)
                .into_iter()
                .filter_map(ChildMetric::of)
                .collect::<Vec<_>>();
            child.id != set.id
                && child
                    .relations
                    .iter()
                    .any(|relation| relation.target == set.id && relation.shift == 0)
                && ChildField::fields(&metrics).contains(&field)
        }
    }
}

fn track_color_applies(set: &SetState, metric: TrackColor) -> bool {
    match metric {
        TrackColor::Measurement(measurement) => measurement.colorable(),
        TrackColor::Relation {
            target,
            field,
            shift,
            ..
        } => {
            RelationField::CONTINUOUS.contains(&field)
                && set
                    .relations
                    .iter()
                    .any(|relation| relation.target == target.raw() && relation.shift == shift)
        }
    }
}

fn fault(detail: &'static str) -> SceneFault {
    SceneFault(detail)
}
