use std::collections::HashMap;

use grana_display::Coloring;
use grana_measure::{ChildField, ColocPair, Group, TrackMeasurement, channel_keys, relation_keys};
use grana_scene::{
    Annotation, AnnotationState, Criteria, FilterRange, LassoPath, MaskFilter, ObjectColor,
    ObjectSetId, ObjectSetKey, Scene, TrackColor,
};
use grana_volume::Region;

use crate::step::Step;
use crate::{ColorSpec, FilterSpec, LassoSpec, Protocol, ProtocolError, SourceSpec};

pub struct Staged {
    sets: Vec<ObjectSetId>,
    pairs: Vec<ColocPair>,
    steps: Vec<Step>,
}

impl Staged {
    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    pub fn set(&self, index: usize) -> ObjectSetId {
        self.sets[index]
    }

    pub fn sets(&self) -> &[ObjectSetId] {
        &self.sets
    }

    pub fn pairs(&self) -> &[ColocPair] {
        &self.pairs
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }
}

struct Builder<'a> {
    scene: &'a mut Scene,
    names: &'a [String],
    ids: HashMap<String, ObjectSetId>,
    steps: Vec<Step>,
    mask_radius: f32,
}

impl Protocol {
    pub fn validate(&self, scene: &Scene) -> Result<(), ProtocolError> {
        let names = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        if !self.channels.is_empty() && self.channels.len() != names.len() {
            return Err(ProtocolError::Invalid(format!(
                "the protocol presents {} channels and the dataset holds {}",
                self.channels.len(),
                names.len()
            )));
        }
        for spec in &self.channels {
            channel(&names, &spec.name)?;
        }
        if let Some(drift) = &self.drift {
            channel(&names, drift.channel())?;
            if drift.reference() >= scene.frame_count() {
                return Err(ProtocolError::Invalid(format!(
                    "the drift correction aligns to frame {}, which the dataset does not hold",
                    drift.reference()
                )));
            }
            if drift.offsets().len() != scene.frame_count() as usize {
                return Err(ProtocolError::Invalid(format!(
                    "the drift correction aligns {} frames and the dataset holds {}",
                    drift.offsets().len(),
                    scene.frame_count()
                )));
            }
            if drift.offsets()[drift.reference() as usize] != [0.0; 3] {
                return Err(ProtocolError::Invalid(
                    "the drift reference frame holds still".to_string(),
                ));
            }
        }
        for spec in &self.sets {
            self.validate_source(&spec.source, &names, scene.frame_count())?;
            if let Some(mask) = &spec.mask
                && !self.sets.iter().any(|other| other.key == *mask)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the set {} is masked by {mask}, which is not an object set of the protocol",
                    spec.name
                )));
            }
            if let Some(pair) = &spec.object_coloc {
                channel(&names, &pair.a)?;
                channel(&names, &pair.b)?;
                if pair.a == pair.b {
                    return Err(ProtocolError::Invalid(format!(
                        "the object colocalization of {} names one channel twice",
                        spec.name
                    )));
                }
            }
        }
        for relation in &self.relations {
            if (i64::from(relation.shift).unsigned_abs() as usize) >= scene.frame_count() as usize {
                return Err(ProtocolError::Invalid(format!(
                    "the relation of {} to {} shifts within the frames of the dataset",
                    relation.source, relation.target
                )));
            }
        }
        if let Some(coloc) = &self.coloc {
            for pair in &coloc.pairs {
                channel(&names, &pair.a)?;
                channel(&names, &pair.b)?;
            }
            if let Some(mask) = &coloc.mask
                && !self.sets.iter().any(|set| set.key == *mask)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the colocalization mask {mask} is not an object set of the protocol"
                )));
            }
        }
        Ok(())
    }

    fn validate_seeds(
        &self,
        seeds: &[Vec<[f32; 3]>],
        seed_source: &Option<String>,
        frames: u32,
    ) -> Result<(), ProtocolError> {
        if !seeds.is_empty() && seeds.len() != frames as usize {
            return Err(ProtocolError::Invalid(
                "the seeds of an object set cover every frame of the dataset".to_string(),
            ));
        }
        if let Some(source) = seed_source
            && !self.sets.iter().any(|set| set.key == *source)
        {
            return Err(ProtocolError::Invalid(format!(
                "the seed source {source} is not an object set of the protocol"
            )));
        }
        Ok(())
    }

    fn validate_source(
        &self,
        source: &SourceSpec,
        names: &[String],
        frames: u32,
    ) -> Result<(), ProtocolError> {
        match source {
            SourceSpec::Spots { channel: name, .. }
            | SourceSpec::Filaments { channel: name, .. } => {
                channel(names, name)?;
            }
            SourceSpec::Segments {
                channel: name,
                seeds,
                seed_source,
                ..
            } => {
                channel(names, name)?;
                self.validate_seeds(seeds, seed_source, frames)?;
            }
            SourceSpec::Surfaces {
                channel: name,
                seeds,
                seed_source,
                ..
            } => {
                channel(names, name)?;
                self.validate_seeds(seeds, seed_source, frames)?;
            }
            SourceSpec::Filtered { source, .. } | SourceSpec::Copy { source } => {
                if !self.sets.iter().any(|set| set.key == *source) {
                    return Err(ProtocolError::Invalid(format!(
                        "{source} is not an object set of the protocol"
                    )));
                }
            }
            SourceSpec::Edited { source, edits } => {
                if !self.sets.iter().any(|set| set.key == *source) {
                    return Err(ProtocolError::Invalid(format!(
                        "{source} is not an object set of the protocol"
                    )));
                }
                for edit in edits {
                    if edit.frame() >= frames {
                        return Err(ProtocolError::Invalid(format!(
                            "the surface edits name frame {} of {frames}",
                            edit.frame()
                        )));
                    }
                }
            }
            SourceSpec::Masked { source, edits } => {
                if !self.sets.iter().any(|set| set.key == *source) {
                    return Err(ProtocolError::Invalid(format!(
                        "{source} is not an object set of the protocol"
                    )));
                }
                for edit in edits {
                    if edit.frame() >= frames {
                        return Err(ProtocolError::Invalid(format!(
                            "the mask edits name frame {} of {frames}",
                            edit.frame()
                        )));
                    }
                }
            }
            SourceSpec::Labels { label } => {
                if label.trim().is_empty() {
                    return Err(ProtocolError::Invalid(
                        "a label import names a label image".to_string(),
                    ));
                }
            }
            SourceSpec::Tracks { file } => {
                if file.trim().is_empty() {
                    return Err(ProtocolError::Invalid(
                        "a track import names a track file".to_string(),
                    ));
                }
            }
            SourceSpec::Merged { parts } => {
                for part in parts {
                    self.validate_source(&part.source, names, frames)?;
                }
            }
        }
        Ok(())
    }

    fn stage_annotations(&self, scene: &mut Scene) {
        let mut entries = Vec::with_capacity(self.annotations.len());
        let mut counts = [0u32; 2];
        for spec in &self.annotations {
            let kind = spec.kind();
            counts[kind.tag().index()] += 1;
            entries.push(
                Annotation::new(spec.name(), spec.frame(), kind).with_visible(spec.visible()),
            );
        }
        scene.annotations.restore(AnnotationState {
            entries,
            selected: None,
            counts,
        });
    }

    pub fn stage(&self, scene: &mut Scene) -> Result<Staged, ProtocolError> {
        assert!(scene.objects.is_empty(), "a protocol stages an empty scene");
        self.validate(scene)?;
        let names = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let keys = channel_keys(&names);
        self.stage_channels(scene, &names)?;
        self.stage_annotations(scene);
        if let Some(render) = self.render {
            scene.exposure = render.exposure;
            scene.step_scale = render.step_scale;
            scene.shading = render.shading();
            scene.set_clip(render.clip());
        }
        let world = scene.dataset.world_size();
        scene.set_region(match self.region {
            Some(region) => region.region(world),
            None => Region::none(),
        });
        scene.set_mask_radius(self.mask_radius);
        let mut builder = Builder {
            scene,
            names: &names,
            ids: HashMap::new(),
            steps: Vec::new(),
            mask_radius: self.mask_radius,
        };
        builder.scene.set_drift(grana_scene::Drift::default());
        if let Some(drift) = &self.drift {
            builder.steps.push(Step::Drift {
                channel: channel(&names, drift.channel())?,
                reference: drift.reference(),
                offsets: drift.offsets().to_vec(),
            });
        }
        let mut sets = Vec::new();
        let mut masks = Vec::new();
        for spec in &self.sets {
            let mask = spec
                .mask
                .as_deref()
                .and_then(|key| builder.ids.get(key).copied());
            let id = builder.build(&spec.source, &spec.name, true, spec.track, mask)?;
            builder.assign_key(id, &spec.key, &spec.name)?;
            {
                let set = builder
                    .scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists");
                set.set_color(spec.color);
                set.set_opacity(spec.opacity);
                set.set_visible(spec.visible);
                set.set_labels(spec.labels);
            }
            if let Some(classifier) = &spec.classifier {
                let set = builder
                    .scene
                    .objects
                    .get(id)
                    .expect("the staged object set exists");
                let classifier = classifier.classifier(set, &keys)?;
                builder.scene.install_classifier(id, classifier);
            }
            if let Some(curation) = &spec.curation {
                builder.apply_curation(id, Some(curation), &spec.name)?;
            }
            if let Some(pair) = &spec.object_coloc {
                let pair = ColocPair::new(channel(&names, &pair.a)?, channel(&names, &pair.b)?);
                builder.steps.push(Step::ObjectColoc { set: id, pair });
            }
            builder.ids.insert(spec.key.clone(), id);
            masks.push(mask);
            sets.push(id);
        }
        for relation in &self.relations {
            let source = builder.ids[&relation.source];
            let target = builder.ids[&relation.target];
            if !builder.scene.add_relation_with_key(
                source,
                target,
                relation.shift,
                relation.key.clone(),
            ) {
                return Err(ProtocolError::Invalid(format!(
                    "{} cannot relate to {} at the offset {}",
                    relation.source, relation.target, relation.shift
                )));
            }
            builder.steps.push(Step::Relation {
                source,
                target,
                shift: relation.shift,
                key: relation.key.clone(),
            });
        }
        let mask_filter = self.mask_filter.as_ref().map(|spec| {
            let source = builder.ids[&spec.source];
            let target = builder.ids[&spec.target];
            let field = spec
                .field()
                .expect("a checked mask filter names a continuous relation field");
            (
                source,
                MaskFilter::new(target, spec.shift, field, spec.minimum, spec.maximum),
            )
        });
        if let Some((source, filter)) = mask_filter {
            for step in &mut builder.steps {
                match step {
                    Step::Detect {
                        mask, filter: slot, ..
                    }
                    | Step::Coloc {
                        mask, filter: slot, ..
                    } if *mask == Some(source) => {
                        *slot = Some(filter);
                    }
                    _ => {}
                }
            }
        }
        for spec in &self.sets {
            let id = builder.ids[&spec.key];
            if let Some(color) = &spec.color_by {
                let coloring = object_coloring(color, builder.scene, id, &keys)?;
                builder.scene.set_object_coloring(id, coloring);
            }
            if let Some(color) = &spec.track_by {
                let coloring = track_coloring(color, builder.scene)?;
                builder
                    .scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_track_coloring(coloring);
            }
            if let Some(label) = &spec.label {
                let set = builder
                    .scene
                    .objects
                    .get(id)
                    .expect("the staged object set exists");
                let metric = label.metric(set, builder.scene, &keys)?;
                builder.scene.set_label_metric(id, Some(metric));
            }
        }
        let mask = masks.iter().rev().flatten().next().copied();
        builder.scene.set_mask(mask);
        if let Some((source, filter)) = mask_filter
            && mask == Some(source)
        {
            builder.scene.set_mask_filter(Some(filter));
        }
        let pairs = match &self.coloc {
            Some(coloc) => coloc
                .pairs
                .iter()
                .map(|pair| {
                    Ok(ColocPair::new(
                        channel(&names, &pair.a)?,
                        channel(&names, &pair.b)?,
                    ))
                })
                .collect::<Result<Vec<_>, ProtocolError>>()?,
            None => Vec::new(),
        };
        if let Some(coloc) = &self.coloc {
            let mask = coloc
                .mask
                .as_deref()
                .and_then(|key| builder.ids.get(key).copied());
            for pair in 0..coloc.pairs.len() {
                builder.steps.push(Step::Coloc {
                    pair,
                    mask,
                    mask_radius: builder.mask_radius,
                    filter: None,
                });
            }
        }
        for key in &self.label_images {
            let set = builder.ids.get(key).copied().ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "the label image of {key} names an object set of the protocol"
                ))
            })?;
            builder.steps.push(Step::Labels { set });
        }
        builder.steps.push(Step::Present);
        let steps = ordered_steps(builder.steps, builder.scene)?;
        Ok(Staged { sets, pairs, steps })
    }

    fn stage_channels(&self, scene: &mut Scene, names: &[String]) -> Result<(), ProtocolError> {
        if self.channels.is_empty() {
            return Ok(());
        }
        if self.channels.len() != names.len() {
            return Err(ProtocolError::Invalid(format!(
                "the protocol presents {} channels and the dataset holds {}",
                self.channels.len(),
                names.len()
            )));
        }
        for spec in &self.channels {
            let index = channel(names, &spec.name)?;
            scene.channels[index].enabled = spec.enabled;
            scene.channels[index].opacity = spec.opacity;
            scene.set_window(index, spec.window.low, spec.window.high);
            if let Some(transfer) = &spec.transfer {
                scene.set_transfer(index, transfer.transfer());
            }
        }
        Ok(())
    }
}

impl Builder<'_> {
    fn build(
        &mut self,
        source: &SourceSpec,
        name: &str,
        named: bool,
        track: Option<crate::TrackSpec>,
        mask: Option<ObjectSetId>,
    ) -> Result<ObjectSetId, ProtocolError> {
        match source {
            SourceSpec::Spots { .. }
            | SourceSpec::Surfaces { .. }
            | SourceSpec::Filaments { .. }
            | SourceSpec::Segments { .. } => self.detect(source, name, named, track, mask),
            SourceSpec::Labels { label } => self.import(label, name, named, track),
            SourceSpec::Tracks { file } => self.tracks(file, name, named, track),
            SourceSpec::Filtered {
                source: reference,
                group,
                filters,
                lasso,
            } => {
                let source = self.reference(reference)?;
                let criteria = criteria(group, filters, lasso.as_ref())?;
                let id = self
                    .scene
                    .filter_object(source, "filtered", criteria.clone())
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "{name} filters an object set that left the scene"
                        ))
                    })?;
                self.rename(id, name, named)?;
                self.apply_track(id, track);
                self.steps.push(Step::Filter {
                    set: id,
                    source,
                    criteria,
                });
                Ok(id)
            }
            SourceSpec::Copy { source: reference } => {
                let source = self.reference(reference)?;
                let id = self.scene.copy_object(source).ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "{name} copies an object set that left the scene"
                    ))
                })?;
                self.rename(id, name, named)?;
                self.apply_track(id, track);
                self.steps.push(Step::Copy { set: id, source });
                Ok(id)
            }
            SourceSpec::Edited {
                source: reference,
                edits,
            } => {
                let source = self.reference(reference)?;
                let plan = edits
                    .iter()
                    .map(|edit| edit.clone().edit())
                    .collect::<Result<Vec<_>, String>>()
                    .map_err(ProtocolError::Invalid)?;
                let id = self
                    .scene
                    .edited_object(source, "edited", plan)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "{name} edits an object set that left the scene"
                        ))
                    })?;
                self.rename(id, name, named)?;
                self.apply_track(id, track);
                self.steps.push(Step::Edit { set: id, source });
                Ok(id)
            }
            SourceSpec::Masked {
                source: reference,
                edits,
            } => {
                let source = self.reference(reference)?;
                let strokes = edits
                    .iter()
                    .map(|edit| edit.clone().edit())
                    .collect::<Result<Vec<_>, String>>()
                    .map_err(ProtocolError::Invalid)?;
                let id = self
                    .scene
                    .masked_object(source, "masked", strokes)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "{name} paints an object set that left the scene"
                        ))
                    })?;
                self.rename(id, name, named)?;
                self.apply_track(id, track);
                self.steps.push(Step::Edit { set: id, source });
                Ok(id)
            }
            SourceSpec::Merged { parts } => {
                let mut built = Vec::with_capacity(parts.len());
                for part in parts {
                    let id = self.build(&part.source, &part.name, false, None, mask)?;
                    self.apply_curation(id, part.curation.as_ref(), &part.name)?;
                    built.push(id);
                }
                let mut built = built.into_iter();
                let target = built
                    .next()
                    .expect("a merged object set holds at least one part");
                self.rename(target, name, named)?;
                self.apply_track(target, track);
                for part in built {
                    self.steps.push(Step::Merge { set: target, part });
                }
                Ok(target)
            }
        }
    }

    fn detect(
        &mut self,
        source: &SourceSpec,
        name: &str,
        named: bool,
        track: Option<crate::TrackSpec>,
        mask: Option<ObjectSetId>,
    ) -> Result<ObjectSetId, ProtocolError> {
        let channel = channel(
            self.names,
            source.channel().expect("a detection names its channel"),
        )?;
        let kind = source.kind().expect("a detection names its object kind");
        let id = match source.segment() {
            Some(params) => self.scene.add_segment(channel, params),
            None => {
                let params = source.params().expect("a detection carries its parameters");
                let id = self.scene.add_object(kind, channel);
                self.scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_params(params);
                id
            }
        };
        self.rename(id, name, named)?;
        if !source.seeds().is_empty() {
            self.scene.set_seed_frames(id, source.seeds().to_vec());
        }
        if let Some(seed_source) = source.seed_source() {
            let seeds = self.reference(seed_source)?;
            if !self.scene.set_seed_source(id, Some(seeds)) {
                return Err(ProtocolError::Invalid(format!(
                    "{seed_source} cannot seed the object set {name}"
                )));
            }
        }
        self.apply_track(id, track);
        self.steps.push(Step::Detect {
            set: id,
            mask,
            mask_radius: self.mask_radius,
            filter: None,
        });
        Ok(id)
    }

    fn reference(&self, key: &str) -> Result<ObjectSetId, ProtocolError> {
        self.ids.get(key).copied().ok_or_else(|| {
            ProtocolError::Invalid(format!("{key} is referenced before the protocol stages it"))
        })
    }

    fn import(
        &mut self,
        label: &str,
        name: &str,
        named: bool,
        track: Option<crate::TrackSpec>,
    ) -> Result<ObjectSetId, ProtocolError> {
        let id = self.scene.import_object(label);
        self.rename(id, name, named)?;
        self.apply_track(id, track);
        self.steps.push(Step::Import { set: id });
        Ok(id)
    }

    fn tracks(
        &mut self,
        file: &str,
        name: &str,
        named: bool,
        track: Option<crate::TrackSpec>,
    ) -> Result<ObjectSetId, ProtocolError> {
        let id = self.scene.import_tracks(file);
        self.rename(id, name, named)?;
        self.apply_track(id, track);
        self.steps.push(Step::Import { set: id });
        Ok(id)
    }

    fn assign_key(&mut self, id: ObjectSetId, key: &str, name: &str) -> Result<(), ProtocolError> {
        let key =
            ObjectSetKey::parse(key).expect("a checked protocol carries a valid object set key");
        let conflict = self
            .scene
            .objects
            .iter()
            .find(|set| set.id() != id && set.key() == &key)
            .map(|set| set.id());
        if let Some(other) = conflict {
            if self.ids.values().any(|top| *top == other) {
                return Err(ProtocolError::Invalid(format!(
                    "the object set {name} collides with another object set key"
                )));
            }
            if !self.scene.rekey_object(other) {
                return Err(ProtocolError::Invalid(format!(
                    "the object set {name} cannot release a staged part key"
                )));
            }
        }
        let current = self.scene.objects.get(id).map(|set| set.key().clone());
        if current.as_ref() == Some(&key) || self.scene.set_object_key(id, key) {
            return Ok(());
        }
        Err(ProtocolError::Invalid(format!(
            "the object set {name} collides with another object set key"
        )))
    }

    fn rename(&mut self, id: ObjectSetId, name: &str, named: bool) -> Result<(), ProtocolError> {
        if !named {
            return Ok(());
        }
        let current = self
            .scene
            .objects
            .get(id)
            .map_or_else(String::new, |set| set.name().to_string());
        if self.scene.rename_object(id, name) || current == name {
            return Ok(());
        }
        Err(ProtocolError::Invalid(format!(
            "the object set {name} collides with another object set of the scene"
        )))
    }

    fn apply_curation(
        &mut self,
        id: ObjectSetId,
        curation: Option<&crate::CurationSpec>,
        name: &str,
    ) -> Result<(), ProtocolError> {
        let Some(curation) = curation else {
            return Ok(());
        };
        if curation.expected.len() != self.scene.dataset.frame_count() as usize {
            return Err(ProtocolError::Invalid(format!(
                "the manual edits of {name} cover {} frames and the dataset holds {}",
                curation.expected.len(),
                self.scene.dataset.frame_count()
            )));
        }
        let curation = curation.curation().map_err(ProtocolError::Invalid)?;
        self.scene.set_curation(id, curation);
        Ok(())
    }

    fn apply_track(&mut self, id: ObjectSetId, track: Option<crate::TrackSpec>) {
        let Some(track) = track else {
            return;
        };
        let Some(kind) = self.scene.objects.get(id).map(|set| set.kind()) else {
            return;
        };
        if !kind.trackable() {
            return;
        }
        let params = grana_object::TrackParams::new(
            track.max_step,
            track.max_gap,
            track.division,
            track.division_range,
        );
        self.scene.set_track_style(id, params, track.thickness);
        if let Some(set) = self.scene.objects.get_mut(id) {
            set.set_track_visible(track.visible);
        }
    }
}

fn criteria(
    group: &str,
    filters: &[FilterSpec],
    lasso: Option<&LassoSpec>,
) -> Result<Criteria, ProtocolError> {
    let group = Group::parse(group).ok_or_else(|| {
        ProtocolError::Invalid("a derived object set filters an unknown grouping".to_string())
    })?;
    Ok(Criteria::new(
        group,
        filters
            .iter()
            .map(|filter| FilterRange::new(&filter.column, filter.minimum, filter.maximum))
            .collect(),
        lasso.and_then(|lasso| LassoPath::new(&lasso.x, &lasso.y, lasso.points.clone())),
    ))
}

fn channel(names: &[String], name: &str) -> Result<usize, ProtocolError> {
    let matches = names
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.as_str() == name)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(ProtocolError::Invalid(format!(
            "the protocol names the channel {name}, which the dataset does not have"
        ))),
        [index] => Ok(*index),
        _ => Err(ProtocolError::Invalid(format!(
            "the dataset names more than one channel {name}"
        ))),
    }
}

fn object_coloring(
    spec: &ColorSpec,
    scene: &Scene,
    id: ObjectSetId,
    keys: &[String],
) -> Result<Coloring<ObjectColor>, ProtocolError> {
    let mut coloring = Coloring::new();
    coloring.set_palette(spec.palette());
    coloring.set_reversed(spec.reversed);
    if let Some([low, high]) = spec.range {
        coloring.set_manual(Some((low, high)));
    }
    if let Some(pair) = &spec.pair {
        let names = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let metric = spec.coloc_metric().ok_or_else(|| {
            ProtocolError::Invalid("an object colocalization color names a measurement".to_string())
        })?;
        coloring.set_metric(Some(ObjectColor::Coloc {
            pair: ColocPair::new(channel(&names, &pair.a)?, channel(&names, &pair.b)?),
            metric,
        }));
        return Ok(coloring);
    }
    let metric = if spec.child {
        let target = spec
            .target
            .as_deref()
            .expect("a checked color names a child set");
        let child = scene
            .objects
            .iter()
            .find(|set| set.key().as_str() == target)
            .ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "the color names the child set {target}, which is not an object set of the scene"
                ))
            })?;
        let key = spec
            .key
            .as_deref()
            .expect("a checked color names a child measurement");
        let field = ChildField::parse(key, spec.aggregate.as_deref(), keys)
            .filter(|field| child.child_fields().contains(field))
            .ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "the set {} has no child measurement {key}",
                    child.name()
                ))
            })?;
        if child.relation(id, 0).is_none() {
            return Err(ProtocolError::Invalid(format!(
                "the child set {} does not relate to the colored set",
                child.name()
            )));
        }
        ObjectColor::Child {
            target: child.id(),
            field,
        }
    } else {
        match (&spec.key, &spec.target) {
            (Some(key), _) if key == grana_measure::SCORE_KEY => {
                let set = scene.objects.get(id).expect("the staged object set exists");
                if set.classifier().is_none() {
                    return Err(ProtocolError::Invalid(format!(
                        "the color names the classifier score of {}, which does not classify",
                        set.name()
                    )));
                }
                ObjectColor::Score
            }
            (Some(key), _) => {
                let set = scene.objects.get(id).expect("the staged object set exists");
                let measurement = set
                    .colorable_measurements()
                    .into_iter()
                    .find(|measurement| measurement.key(keys) == *key)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the set {} has no colorable measurement {key}",
                            set.name()
                        ))
                    })?;
                ObjectColor::Measurement(measurement)
            }
            (_, Some(target)) => {
                let target = scene
                    .objects
                    .iter()
                    .find(|set| set.key().as_str() == target)
                    .map(|set| set.id())
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the color names the relation target {target}, which is not an object set of the scene"
                        ))
                    })?;
                ObjectColor::Relation {
                    target,
                    field: spec
                        .field()
                        .expect("a checked color names a relation field"),
                    shift: spec.shift,
                }
            }
            _ => unreachable!("a checked color names one measurement or relation"),
        }
    };
    coloring.set_metric(Some(metric));
    Ok(coloring)
}

fn track_coloring(spec: &ColorSpec, scene: &Scene) -> Result<Coloring<TrackColor>, ProtocolError> {
    let mut coloring = Coloring::new();
    coloring.set_palette(spec.palette());
    coloring.set_reversed(spec.reversed);
    if let Some([low, high]) = spec.range {
        coloring.set_manual(Some((low, high)));
    }
    let metric = match (&spec.key, &spec.target) {
        (Some(key), None) => {
            let measurement = TrackMeasurement::ALL
                .into_iter()
                .filter(|measurement| measurement.colorable())
                .find(|measurement| measurement.column().key() == key.as_str())
                .ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "the tracks have no colorable measurement {key}"
                    ))
                })?;
            TrackColor::Measurement(measurement)
        }
        (None, Some(target)) => {
            let target = scene
                .objects
                .iter()
                .find(|set| set.key().as_str() == target)
                .map(|set| set.id())
                .ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "the color names the relation target {target}, which is not an object set of the scene"
                    ))
                })?;
            TrackColor::Relation {
                target,
                field: spec
                    .field()
                    .expect("a checked color names a relation field"),
                aggregate: spec.aggregate().unwrap_or(grana_measure::Aggregate::Mean),
                shift: spec.shift,
            }
        }
        _ => unreachable!("a checked color names one measurement or relation"),
    };
    coloring.set_metric(Some(metric));
    Ok(coloring)
}

fn ordered_steps(steps: Vec<Step>, scene: &Scene) -> Result<Vec<Step>, ProtocolError> {
    let count = steps.len();
    let drift = steps
        .iter()
        .position(|step| matches!(step, Step::Drift { .. }));
    let mut ready = HashMap::<ObjectSetId, usize>::new();
    let mut base = HashMap::<ObjectSetId, usize>::new();
    for (index, step) in steps.iter().enumerate() {
        let Some(set) = step.installs() else {
            continue;
        };
        ready.insert(set, index);
        if !matches!(step, Step::Merge { .. }) {
            base.insert(set, index);
        }
    }
    let mut dependencies = vec![Vec::new(); count];
    let mut merges = HashMap::<ObjectSetId, usize>::new();
    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Drift { .. } => {}
            Step::Import { .. } => dependencies[index].extend(drift),
            Step::Detect {
                set, mask, filter, ..
            } => {
                dependencies[index].extend(drift);
                if let Some(mask) = mask {
                    dependencies[index].push(settled(&ready, *mask)?);
                }
                if let (Some(mask), Some(filter)) = (mask, filter) {
                    dependencies[index].push(filter_relation(&steps, *mask, *filter)?);
                }
                if let Some(seed) = scene.seed_source(*set) {
                    dependencies[index].push(settled(&ready, seed)?);
                }
            }
            Step::Filter {
                source, criteria, ..
            } => {
                dependencies[index].push(settled(&ready, *source)?);
                let keys = criteria_keys(criteria);
                for (relation, step) in steps.iter().enumerate() {
                    let Step::Relation {
                        source: related,
                        key,
                        ..
                    } = step
                    else {
                        continue;
                    };
                    if related != source {
                        continue;
                    }
                    if relation_keys(key)
                        .iter()
                        .any(|candidate| keys.contains(candidate))
                    {
                        dependencies[index].push(relation);
                    }
                }
            }
            Step::Copy { source, .. } => dependencies[index].push(settled(&ready, *source)?),
            Step::Edit { source, .. } => dependencies[index].push(settled(&ready, *source)?),
            Step::Merge { set, part } => {
                dependencies[index].push(settled(&ready, *part)?);
                match merges.insert(*set, index) {
                    Some(previous) => dependencies[index].push(previous),
                    None => {
                        dependencies[index].push(*base.get(set).ok_or_else(|| unproduced(*set))?)
                    }
                }
            }
            Step::Relation { source, target, .. } => {
                dependencies[index].push(settled(&ready, *source)?);
                dependencies[index].push(settled(&ready, *target)?);
            }
            Step::ObjectColoc { set, .. } => {
                dependencies[index].extend(drift);
                dependencies[index].push(settled(&ready, *set)?);
            }
            Step::Labels { set } => {
                dependencies[index].push(settled(&ready, *set)?);
            }
            Step::Coloc { mask, filter, .. } => {
                dependencies[index].extend(drift);
                if let Some(mask) = mask {
                    dependencies[index].push(settled(&ready, *mask)?);
                }
                if let (Some(mask), Some(filter)) = (mask, filter) {
                    dependencies[index].push(filter_relation(&steps, *mask, *filter)?);
                }
            }
            Step::Present => dependencies[index].extend(0..index),
        }
    }
    let mut emitted = vec![false; count];
    let mut ordered = Vec::with_capacity(count);
    while ordered.len() < count {
        let Some(next) = (0..count).find(|index| {
            !emitted[*index]
                && dependencies[*index]
                    .iter()
                    .all(|dependency| emitted[*dependency])
        }) else {
            return Err(ProtocolError::Invalid(
                "the protocol relates object sets that wait for the relation itself".to_string(),
            ));
        };
        emitted[next] = true;
        ordered.push(steps[next].clone());
    }
    Ok(ordered)
}

fn criteria_keys(criteria: &Criteria) -> Vec<String> {
    let mut keys = criteria
        .filters()
        .iter()
        .map(|filter| filter.key().to_string())
        .collect::<Vec<_>>();
    if let Some(lasso) = criteria.lasso() {
        keys.push(lasso.x().to_string());
        keys.push(lasso.y().to_string());
    }
    keys
}

fn settled(ready: &HashMap<ObjectSetId, usize>, set: ObjectSetId) -> Result<usize, ProtocolError> {
    ready.get(&set).copied().ok_or_else(|| unproduced(set))
}

fn filter_relation(
    steps: &[Step],
    mask: ObjectSetId,
    filter: MaskFilter,
) -> Result<usize, ProtocolError> {
    steps
        .iter()
        .position(|step| {
            matches!(
                step,
                Step::Relation { source, target, shift, .. }
                    if *source == mask && *target == filter.target() && *shift == filter.shift()
            )
        })
        .ok_or_else(|| {
            ProtocolError::Invalid(
                "the analysis mask filters its objects by a relation the protocol does not measure"
                    .to_string(),
            )
        })
}

fn unproduced(set: ObjectSetId) -> ProtocolError {
    ProtocolError::Invalid(format!(
        "the object set {} has no step that produces it",
        set.raw()
    ))
}
