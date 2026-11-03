use grana_display::Coloring;
use grana_measure::{ColocPair, TrackMeasurement, channel_keys};
use grana_scene::{ObjectColor, ObjectSetId, Scene, TrackColor};
use grana_volume::Region;

use super::{ColorSpec, Protocol, ProtocolError};

pub struct Staged {
    sets: Vec<ObjectSetId>,
    pairs: Vec<ColocPair>,
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
        for spec in &self.sets {
            channel(&names, &spec.channel)?;
            if !spec.seeds.is_empty() && spec.seeds.len() != scene.frame_count() as usize {
                return Err(ProtocolError::Invalid(format!(
                    "the seeds of {} cover every frame of the dataset",
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
        }
        let world = scene.dataset.world_size();
        scene.set_region(match self.region {
            Some(region) => region.region(world),
            None => Region::none(),
        });
        let order = self
            .sets
            .iter()
            .map(|set| set.name.as_str())
            .collect::<Vec<_>>();
        let mut sets = Vec::new();
        let mut masks = Vec::new();
        for spec in &self.sets {
            let channel = channel(&names, &spec.channel)?;
            let id = scene.add_object(spec.kind(), channel);
            if !scene.rename_object(id, &spec.name)
                && !scene
                    .objects
                    .get(id)
                    .is_some_and(|set| set.name() == spec.name)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the object set {} collides with another object set of the scene",
                    spec.name
                )));
            }
            {
                let set = scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists");
                set.set_color(spec.color);
                set.set_opacity(spec.opacity);
                set.set_visible(spec.visible);
                set.set_params(spec.params());
            }
            if let (Some(params), Some(thickness), Some(visible)) =
                (spec.track_params(), spec.thickness(), spec.track_visible())
            {
                scene.set_track_style(id, params, thickness);
                scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_track_visible(visible);
            }
            if !spec.seeds.is_empty() {
                if spec.seeds.len() != scene.frame_count() as usize {
                    return Err(ProtocolError::Invalid(format!(
                        "the seeds of {} cover every frame of the dataset",
                        spec.name
                    )));
                }
                scene.set_seed_frames(id, spec.seeds.clone());
            }
            masks.push(match spec.mask() {
                Some(mask) => Some(position(&order, mask, &spec.name)?),
                None => None,
            });
            sets.push(id);
        }
        for relation in &self.relations {
            let source = position(&order, &relation.source, &relation.source)?;
            let target = position(&order, &relation.target, &relation.target)?;
            if !scene.add_relation(sets[source], sets[target], relation.shift) {
                return Err(ProtocolError::Invalid(format!(
                    "{} cannot relate to {}",
                    relation.source, relation.target
                )));
            }
        }
        for (index, spec) in self.sets.iter().enumerate() {
            let id = sets[index];
            if let Some(color) = &spec.color_by {
                let coloring = object_coloring(color, scene, id, &keys)?;
                scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_object_coloring(coloring);
            }
            if let Some(color) = &spec.track_by {
                let coloring = track_coloring(color, scene)?;
                scene
                    .objects
                    .get_mut(id)
                    .expect("the staged object set exists")
                    .set_track_coloring(coloring);
            }
        }
        let mask = masks.iter().rev().flatten().next().copied();
        scene.set_mask(mask.map(|index| sets[index]));
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
        Ok(Staged { sets, pairs })
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

fn position(names: &[&str], name: &str, set: &str) -> Result<usize, ProtocolError> {
    names
        .iter()
        .position(|candidate| *candidate == name)
        .ok_or_else(|| {
            ProtocolError::Invalid(format!(
                "the set {set} names {name}, which is not an object set of the protocol"
            ))
        })
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
            }
        }
        _ => unreachable!("a checked color names one measurement or relation"),
    };
    coloring.set_metric(Some(metric));
    Ok(coloring)
}
