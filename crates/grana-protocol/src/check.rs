use grana_measure::{COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX};
use grana_scene::ObjectSetKey;

use crate::spec::{SourceSpec, validate};
use crate::{DerivedSpec, Protocol, ProtocolError};

impl Protocol {
    pub(crate) fn checked(self) -> Result<Self, ProtocolError> {
        if self.sets.is_empty() {
            return Err(ProtocolError::Invalid(
                "a protocol names at least one object set".to_string(),
            ));
        }
        if let Some(region) = &self.region {
            region.checked()?;
        }
        if !(self.mask_radius.is_finite() && self.mask_radius >= 0.0) {
            return Err(ProtocolError::Invalid(
                "an analysis mask grows by a finite radius".to_string(),
            ));
        }
        let mut annotation_names: Vec<String> = Vec::new();
        for annotation in &self.annotations {
            annotation.checked()?;
            if annotation_names
                .iter()
                .any(|name| name == annotation.name())
            {
                return Err(ProtocolError::Invalid(format!(
                    "the annotation {} appears twice",
                    annotation.name()
                )));
            }
            annotation_names.push(annotation.name().to_string());
        }
        if let Some(render) = self.render {
            render.checked()?;
        }
        for (index, channel) in self.channels.iter().enumerate() {
            channel.checked()?;
            if self.channels[..index]
                .iter()
                .any(|other| other.name == channel.name)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the channel {} appears twice",
                    channel.name
                )));
            }
        }
        if !self.derived.is_empty() {
            if self.channels.len() < self.derived.len() {
                return Err(ProtocolError::Invalid(
                    "a protocol presents every derived channel".to_string(),
                ));
            }
            let base = self.channels.len() - self.derived.len();
            for (index, spec) in self.derived.iter().enumerate() {
                spec.checked()?;
                if self.derived[..index]
                    .iter()
                    .any(|other| other.name() == spec.name())
                {
                    return Err(ProtocolError::Invalid(format!(
                        "the derived channel {} appears twice",
                        spec.name()
                    )));
                }
                if self.channels[base + index].name != spec.name() {
                    return Err(ProtocolError::Invalid(format!(
                        "the derived channel {} does not close the channel list",
                        spec.name()
                    )));
                }
                if let DerivedSpec::Mask(mask) = spec
                    && !self.sets.iter().any(|set| set.key == mask.set)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "the mask channel {} masks an object set of the protocol",
                        mask.name
                    )));
                }
                let Some(source) = self
                    .channels
                    .iter()
                    .position(|channel| channel.name == spec.channel())
                else {
                    return Err(ProtocolError::Invalid(format!(
                        "the derived channel {} derives from the missing channel {}",
                        spec.name(),
                        spec.channel()
                    )));
                };
                if source >= base + index {
                    return Err(ProtocolError::Invalid(format!(
                        "the derived channel {} derives from a channel that follows it",
                        spec.name()
                    )));
                }
                if let DerivedSpec::Ratio(ratio) = spec {
                    let Some(denominator) = self
                        .channels
                        .iter()
                        .position(|channel| channel.name == ratio.denominator)
                    else {
                        return Err(ProtocolError::Invalid(format!(
                            "the ratio channel {} divides the missing channel {}",
                            spec.name(),
                            ratio.denominator
                        )));
                    };
                    if denominator >= base + index || denominator == source {
                        return Err(ProtocolError::Invalid(format!(
                            "the ratio channel {} divides a channel that follows it",
                            spec.name()
                        )));
                    }
                }
            }
        }
        let mut names = Vec::new();
        let mut set_keys = Vec::new();
        let protocol_keys = self
            .sets
            .iter()
            .map(|set| set.key.as_str())
            .collect::<Vec<_>>();
        if let Some(drift) = &self.drift {
            validate(&drift.channel, "a channel name")?;
            if !self.channels.is_empty() && !self.channels.iter().any(|c| c.name == drift.channel) {
                return Err(ProtocolError::Invalid(format!(
                    "the drift correction aligns the channel {}, which the protocol does not present",
                    drift.channel
                )));
            }
            if drift.offsets.is_empty() {
                return Err(ProtocolError::Invalid(
                    "the drift correction carries no frame offsets".to_string(),
                ));
            }
            if drift.reference as usize >= drift.offsets.len() {
                return Err(ProtocolError::Invalid(format!(
                    "the drift reference frame {} is not one of the aligned frames",
                    drift.reference
                )));
            }
            if drift.offsets[drift.reference as usize] != [0.0; 3] {
                return Err(ProtocolError::Invalid(
                    "the drift reference frame holds still".to_string(),
                ));
            }
            for offset in &drift.offsets {
                if offset
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > grana_scene::Drift::LIMIT)
                {
                    return Err(ProtocolError::Invalid(
                        "a drift offset stays inside the volume".to_string(),
                    ));
                }
            }
        }
        for set in &self.sets {
            validate(&set.name, "a set name")?;
            if let Some(material) = set.material {
                material.checked(&set.name)?;
            }
            if ObjectSetKey::parse(&set.key).is_none() {
                return Err(ProtocolError::Invalid(format!(
                    "the set {} carries an invalid key {}",
                    set.name, set.key
                )));
            }
            if set_keys.contains(&set.key) {
                return Err(ProtocolError::Invalid(format!(
                    "the set key {} appears twice",
                    set.key
                )));
            }
            if names.contains(&set.name) {
                return Err(ProtocolError::Invalid(format!(
                    "the set name {} appears twice",
                    set.name
                )));
            }
            set.source.checked(&set.name)?;
            self.check_parts(&set.source)?;
            self.dependencies(&set.source, &set_keys, &set.name)?;
            if !self.source_kind_consistent(&set.key) {
                return Err(ProtocolError::Invalid(format!(
                    "{} merges object sets of another kind",
                    set.name
                )));
            }
            if let Some(mask) = &set.mask {
                validate(mask, "an analysis mask")?;
                if !set_keys.contains(mask) {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is masked by an object set that does not run before it",
                        set.name
                    )));
                }
            }
            if set.color.iter().any(|value| !(0.0..=1.0).contains(value)) {
                return Err(ProtocolError::Invalid(format!(
                    "the color of {} is not an srgb triple",
                    set.name
                )));
            }
            for (index, mesh) in set.meshes.iter().enumerate() {
                if *mesh == grana_mesh::Format::Obj || set.meshes[..index].contains(mesh) {
                    return Err(ProtocolError::Invalid(format!(
                        "{} exports {} twice or as OBJ",
                        set.name,
                        mesh.label()
                    )));
                }
            }
            if !(0.0..=1.0).contains(&set.opacity) {
                return Err(ProtocolError::Invalid(format!(
                    "the opacity of {} is a unit fraction",
                    set.name
                )));
            }
            if let Some(classifier) = &set.classifier {
                classifier.checked(&set.name)?;
            }
            if let Some(pair) = &set.object_coloc {
                for name in [&pair.a, &pair.b] {
                    validate(name, "an object colocalization channel")?;
                    if !self.channels.is_empty()
                        && !self.channels.iter().any(|channel| channel.name == *name)
                    {
                        return Err(ProtocolError::Invalid(format!(
                            "{} colocalizes the missing channel {name}",
                            set.name
                        )));
                    }
                }
                if pair.a == pair.b {
                    return Err(ProtocolError::Invalid(format!(
                        "{} colocalizes one channel with itself",
                        set.name
                    )));
                }
            }
            if let Some(color) = &set.color_by {
                match (&color.pair, &set.object_coloc) {
                    (Some(pair), Some(measured)) if pair != measured => {
                        return Err(ProtocolError::Invalid(format!(
                            "{} is colored by a colocalization it does not measure",
                            set.name
                        )));
                    }
                    (Some(_), None) => {
                        return Err(ProtocolError::Invalid(format!(
                            "{} is colored by an object colocalization it does not measure",
                            set.name
                        )));
                    }
                    _ => {}
                }
            }
            if let Some(color) = &set.color_by {
                color.checked(&set.name)?;
                if color.key.as_deref() == Some(grana_measure::SCORE_KEY)
                    && set.classifier.is_none()
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is colored by a classifier score without a classifier",
                        set.name
                    )));
                }
                if let Some(target) = &color.target
                    && !protocol_keys.contains(&target.as_str())
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is colored by {}, which is not an object set of the protocol",
                        set.name, target
                    )));
                }
            }
            if let Some(label) = &set.label {
                label.checked(&set.name)?;
                if label.key.as_deref() == Some(grana_measure::SCORE_KEY)
                    && set.classifier.is_none()
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} labels its objects by a classifier score without a classifier",
                        set.name
                    )));
                }
                if let Some(target) = &label.target
                    && !protocol_keys.contains(&target.as_str())
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} labels its objects by {}, which is not an object set of the protocol",
                        set.name, target
                    )));
                }
            }
            if let Some(color) = &set.track_by {
                color.checked(&format!("the tracks of {}", set.name))?;
                if color.child {
                    return Err(ProtocolError::Invalid(format!(
                        "{} colors its tracks by the children of an object",
                        set.name
                    )));
                }
                if set.track.is_none() {
                    return Err(ProtocolError::Invalid(format!(
                        "{} carries track colors without tracking",
                        set.name
                    )));
                }
            }
            match (self.kind_of(&set.key), &set.track) {
                (Some(kind), Some(track)) => {
                    if !kind.trackable() {
                        return Err(ProtocolError::Invalid(format!(
                            "{} holds untrackable objects and cannot be tracked",
                            set.name
                        )));
                    }
                    track.checked(&set.name)?;
                }
                (Some(_), None) => {}
                (None, _) => {
                    return Err(ProtocolError::Invalid(format!(
                        "{} does not resolve to an object kind",
                        set.name
                    )));
                }
            }
            if let Some(measure) = &set.measure {
                measure.checked(&set.name, set.track.is_some())?;
            }
            if let Some(curation) = &set.curation {
                curation.checked(&set.name, set.track.is_some())?;
            }
            names.push(set.name.clone());
            set_keys.push(set.key.clone());
        }
        let measured = self.sets.iter().filter(|set| set.measure.is_some()).count();
        if measured > 1 {
            return Err(ProtocolError::Invalid(
                "a protocol measures one object set".to_string(),
            ));
        }
        for (index, key) in self.label_images.iter().enumerate() {
            if !set_keys.contains(key) {
                return Err(ProtocolError::Invalid(format!(
                    "the label image {key} names an object set of the protocol"
                )));
            }
            if self.label_images[..index].contains(key) {
                return Err(ProtocolError::Invalid(format!(
                    "the label image {key} is exported twice"
                )));
            }
        }
        let mut relations = Vec::new();
        let mut keys: Vec<(String, String)> = Vec::new();
        for relation in &self.relations {
            validate(&relation.source, "a relation source")?;
            validate(&relation.target, "a relation target")?;
            validate(&relation.key, "a relation key")?;
            if keys
                .iter()
                .any(|(source, key)| *source == relation.source && *key == relation.key)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the relation key {} appears twice under {}",
                    relation.key, relation.source
                )));
            }
            keys.push((relation.source.clone(), relation.key.clone()));
            if !set_keys.contains(&relation.source) {
                return Err(ProtocolError::Invalid(format!(
                    "the relation source {} is not an object set of the protocol",
                    relation.source
                )));
            }
            if !set_keys.contains(&relation.target) {
                return Err(ProtocolError::Invalid(format!(
                    "the relation target {} is not an object set of the protocol",
                    relation.target
                )));
            }
            let key = (&relation.source, &relation.target, relation.shift);
            if relations.contains(&key) {
                return Err(ProtocolError::Invalid(format!(
                    "{} relates to {} at the offset {} twice",
                    relation.source, relation.target, relation.shift
                )));
            }
            relations.push(key);
        }
        if let Some(filter) = &self.mask_filter {
            filter.checked()?;
            if !set_keys.contains(&filter.source) || !set_keys.contains(&filter.target) {
                return Err(ProtocolError::Invalid(
                    "an analysis mask filter names object sets of the protocol".to_string(),
                ));
            }
            if !relations.contains(&(&filter.source, &filter.target, filter.shift)) {
                return Err(ProtocolError::Invalid(format!(
                    "the analysis mask filters {} by a relation to {} at the offset {} that the protocol does not measure",
                    filter.source, filter.target, filter.shift
                )));
            }
            if self.sets.iter().any(|set| {
                set.key == filter.target && set.mask.as_deref() == Some(filter.source.as_str())
            }) {
                return Err(ProtocolError::Invalid(format!(
                    "the relation target {} cannot be detected inside the mask that its relation limits",
                    filter.target
                )));
            }
        }
        for set in &self.sets {
            for color in [set.color_by.as_ref(), set.track_by.as_ref()]
                .into_iter()
                .flatten()
            {
                let Some(target) = &color.target else {
                    continue;
                };
                let measured = if color.child {
                    relations.contains(&(target, &set.key, 0))
                } else {
                    relations.contains(&(&set.key, target, color.shift))
                };
                if !measured {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is colored by {} at the offset {} that the protocol does not measure",
                        set.name, target, color.shift
                    )));
                }
            }
        }
        for set in &self.sets {
            let Some(label) = &set.label else {
                continue;
            };
            let Some(target) = &label.target else {
                continue;
            };
            let measured = if label.child {
                relations.contains(&(target, &set.key, 0))
            } else {
                relations.contains(&(&set.key, target, label.shift))
            };
            if !measured {
                return Err(ProtocolError::Invalid(format!(
                    "{} labels its objects by {} at the offset {} that the protocol does not measure",
                    set.name, target, label.shift
                )));
            }
        }
        if let Some(coloc) = &self.coloc {
            if !(1..=COLOC_PERMUTATIONS_MAX).contains(&coloc.settings.permutations) {
                return Err(ProtocolError::Invalid(
                    "a colocalization significance test runs a supported number of permutations"
                        .to_string(),
                ));
            }
            if !(1..=COLOC_BLOCK_MAX).contains(&coloc.settings.block) {
                return Err(ProtocolError::Invalid(
                    "a colocalization randomization block spans a supported number of voxels"
                        .to_string(),
                ));
            }
            if coloc.pairs.is_empty() {
                return Err(ProtocolError::Invalid(
                    "a colocalization specification names at least one channel pair".to_string(),
                ));
            }
            let mut pairs = Vec::new();
            for pair in &coloc.pairs {
                validate(&pair.a, "a channel name")?;
                validate(&pair.b, "a channel name")?;
                if pair.a == pair.b {
                    return Err(ProtocolError::Invalid(
                        "a colocalization pair names two channels".to_string(),
                    ));
                }
                if pair
                    .thresholds
                    .iter()
                    .any(|value| !(0.0..=1.0).contains(value))
                {
                    return Err(ProtocolError::Invalid(
                        "a colocalization threshold is a unit fraction".to_string(),
                    ));
                }
                let key = (&pair.a, &pair.b);
                if pairs.contains(&key) {
                    return Err(ProtocolError::Invalid(format!(
                        "{} and {} are colocalized twice",
                        pair.a, pair.b
                    )));
                }
                pairs.push(key);
            }
            if let Some(mask) = &coloc.mask {
                validate(mask, "a colocalization mask")?;
                if !set_keys.contains(mask) {
                    return Err(ProtocolError::Invalid(format!(
                        "the colocalization mask {mask} is not an object set of the protocol"
                    )));
                }
            }
        }
        if let Some(view) = &self.view {
            view.checked()?;
        }
        for (index, view) in self.views.iter().enumerate() {
            view.checked()?;
            if self.views[..index]
                .iter()
                .any(|other| other.name == view.name)
            {
                return Err(ProtocolError::Invalid(format!(
                    "the named view {} appears twice",
                    view.name
                )));
            }
        }
        if let Some(snapshot) = &self.snapshot {
            snapshot.checked()?;
            for name in &snapshot.views {
                if self.named_view(name).is_none() {
                    return Err(ProtocolError::Invalid(format!(
                        "the snapshot renders the missing view {name}"
                    )));
                }
            }
        }
        Ok(self)
    }

    fn check_parts(&self, source: &SourceSpec) -> Result<(), ProtocolError> {
        let SourceSpec::Merged { parts } = source else {
            return Ok(());
        };
        for part in parts {
            if let Some(curation) = &part.curation {
                curation.checked(&part.name, true)?;
            }
            self.check_parts(&part.source)?;
        }
        Ok(())
    }

    fn dependencies(
        &self,
        source: &SourceSpec,
        earlier: &[String],
        set: &str,
    ) -> Result<(), ProtocolError> {
        if let Some(channel) = source.channel()
            && let Some(mask) = self.derived.iter().find_map(|spec| match spec {
                DerivedSpec::Mask(mask) if mask.name == channel => Some(mask),
                _ => None,
            })
            && !earlier.contains(&mask.set)
        {
            return Err(ProtocolError::Invalid(format!(
                "{set} detects on the mask channel {channel}, whose object set {} does not run before it",
                mask.set
            )));
        }
        match source {
            SourceSpec::Surfaces { seed_source, .. } | SourceSpec::Segments { seed_source, .. } => {
                if let Some(source) = seed_source
                    && !earlier.contains(source)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{set} seeds from {source}, which does not run before it"
                    )));
                }
                Ok(())
            }
            SourceSpec::Spots { .. }
            | SourceSpec::Filaments { .. }
            | SourceSpec::Labels { .. }
            | SourceSpec::Tracks { .. } => Ok(()),
            SourceSpec::Filtered { source, .. }
            | SourceSpec::Copy { source }
            | SourceSpec::Edited { source, .. }
            | SourceSpec::Masked { source, .. } => {
                if !earlier.contains(source) {
                    return Err(ProtocolError::Invalid(format!(
                        "{set} derives from {source}, which does not run before it"
                    )));
                }
                Ok(())
            }
            SourceSpec::Merged { parts } => {
                for part in parts {
                    self.dependencies(&part.source, earlier, set)?;
                }
                Ok(())
            }
        }
    }
}
