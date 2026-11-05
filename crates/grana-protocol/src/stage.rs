use std::collections::HashMap;

use grana_display::Coloring;
use grana_measure::{ColocPair, Group, TrackMeasurement, channel_keys, relation_keys};
use grana_scene::{Criteria, FilterRange, LassoPath, ObjectColor, ObjectSetId, Scene, TrackColor};
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
        }
        for spec in &self.sets {
            self.validate_source(&spec.source, &names, scene.frame_count())?;
            if let Some(mask) = &spec.mask
                && !self.sets.iter().any(|other| other.name == *mask)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the set {} is masked by {mask}, which is not an object set of the protocol",
                    spec.name
                )));
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
                && !self.sets.iter().any(|set| set.name == *mask)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the colocalization mask {mask} is not an object set of the protocol"
                )));
            }
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
            SourceSpec::Surfaces {
                channel: name,
                seeds,
                seed_source,
                ..
            } => {
                channel(names, name)?;
                if !seeds.is_empty() && seeds.len() != frames as usize {
                    return Err(ProtocolError::Invalid(
                        "the seeds of an object set cover every frame of the dataset".to_string(),
                    ));
                }
                if let Some(source) = seed_source
                    && !self.sets.iter().any(|set| set.name == *source)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "the seed source {source} is not an object set of the protocol"
                    )));
                }
            }
            SourceSpec::Filtered { source, .. } | SourceSpec::Copy { source } => {
                if !self.sets.iter().any(|set| set.name == *source) {
                    return Err(ProtocolError::Invalid(format!(
                        "{source} is not an object set of the protocol"
                    )));
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
        let mut builder = Builder {
            scene,
            names: &names,
            ids: HashMap::new(),
            steps: Vec::new(),
        };
        builder.scene.set_drift(grana_scene::Drift::default());
        if let Some(drift) = &self.drift {
            builder.steps.push(Step::Drift {
                channel: channel(&names, drift.channel())?,
                reference: drift.reference(),
            });
        }
        let mut sets = Vec::new();
        let mut masks = Vec::new();
        for spec in &self.sets {
            let mask = spec
                .mask
                .as_deref()
                .and_then(|name| builder.ids.get(name).copied());
            let id = builder.build(&spec.source, &spec.name, true, spec.track, mask)?;
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
            if let Some(curation) = &spec.curation {
                builder.apply_curation(id, Some(curation), &spec.name)?;
            }
            builder.ids.insert(spec.name.clone(), id);
            masks.push(mask);
            sets.push(id);
        }
        for relation in &self.relations {
            let source = builder.ids[&relation.source];
            let target = builder.ids[&relation.target];
            if !builder.scene.add_relation(source, target, relation.shift) {
                return Err(ProtocolError::Invalid(format!(
                    "{} cannot relate to {} at the offset {}",
                    relation.source, relation.target, relation.shift
                )));
            }
            builder.steps.push(Step::Relation {
                source,
                target,
                shift: relation.shift,
            });
        }
        for spec in &self.sets {
            let id = builder.ids[&spec.name];
            if let Some(color) = &spec.color_by {
                let coloring = object_coloring(color, builder.scene, id, &keys)?;
                builder
                    .scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_object_coloring(coloring);
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
                builder
                    .scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_label_metric(Some(metric));
            }
        }
        let mask = masks.iter().rev().flatten().next().copied();
        builder.scene.set_mask(mask);
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
                .and_then(|name| builder.ids.get(name).copied());
            for pair in 0..coloc.pairs.len() {
                builder.steps.push(Step::Coloc { pair, mask });
            }
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
            | SourceSpec::Filaments { .. } => self.detect(source, name, named, track, mask),
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
        let params = source.params().expect("a detection carries its parameters");
        let id = self.scene.add_object(kind, channel);
        self.rename(id, name, named)?;
        self.scene
            .objects
            .get_mut(id)
            .expect("the staged object set exists")
            .set_params(params);
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
        self.steps.push(Step::Detect { set: id, mask });
        Ok(id)
    }

    fn reference(&self, name: &str) -> Result<ObjectSetId, ProtocolError> {
        self.ids.get(name).copied().ok_or_else(|| {
            ProtocolError::Invalid(format!(
                "{name} is referenced before the protocol stages it"
            ))
        })
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
    let metric = match (&spec.key, &spec.target) {
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
            let names = scene
                .objects
                .iter()
                .map(|set| (set.id(), set.name().to_string()))
                .collect::<Vec<_>>();
            let target = names
                .iter()
                .find(|(_, name)| name == target)
                .map(|(id, _)| *id)
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
                .find(|set| set.name() == target)
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
            Step::Detect { set, mask } => {
                dependencies[index].extend(drift);
                if let Some(mask) = mask {
                    dependencies[index].push(settled(&ready, *mask)?);
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
                        target,
                        shift,
                    } = step
                    else {
                        continue;
                    };
                    if related != source {
                        continue;
                    }
                    let name = scene
                        .objects
                        .get(*target)
                        .map_or(String::new(), |set| set.name().to_string());
                    if relation_keys(&name, *shift)
                        .iter()
                        .any(|key| keys.contains(key))
                    {
                        dependencies[index].push(relation);
                    }
                }
            }
            Step::Copy { source, .. } => dependencies[index].push(settled(&ready, *source)?),
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
            Step::Coloc { mask, .. } => {
                dependencies[index].extend(drift);
                if let Some(mask) = mask {
                    dependencies[index].push(settled(&ready, *mask)?);
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

fn unproduced(set: ObjectSetId) -> ProtocolError {
    ProtocolError::Invalid(format!(
        "the object set {} has no step that produces it",
        set.raw()
    ))
}
