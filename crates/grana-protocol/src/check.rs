use grana_measure::{COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX};
use grana_scene::slug;

use crate::spec::{SourceSpec, validate};
use crate::{Protocol, ProtocolError};

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
        let mut names = Vec::new();
        let mut slugs = Vec::new();
        for set in &self.sets {
            validate(&set.name, "a set name")?;
            if names.contains(&set.name) {
                return Err(ProtocolError::Invalid(format!(
                    "the set name {} appears twice",
                    set.name
                )));
            }
            let key = slug(&set.name).to_ascii_lowercase();
            if slugs.contains(&key) {
                return Err(ProtocolError::Invalid(format!(
                    "the set name {} collides with another output name",
                    set.name
                )));
            }
            set.source.checked(&set.name)?;
            self.check_parts(&set.source)?;
            self.dependencies(&set.source, &names, &set.name)?;
            if !self.source_kind_consistent(&set.name) {
                return Err(ProtocolError::Invalid(format!(
                    "{} merges object sets of another kind",
                    set.name
                )));
            }
            if let Some(mask) = &set.mask {
                validate(mask, "an analysis mask")?;
                if !names.contains(mask) {
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
            if !(0.0..=1.0).contains(&set.opacity) {
                return Err(ProtocolError::Invalid(format!(
                    "the opacity of {} is a unit fraction",
                    set.name
                )));
            }
            if let Some(color) = &set.color_by {
                color.checked(&set.name)?;
                if let Some(target) = &color.target
                    && !names.contains(target)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is colored by a relation to {target}, which is not an object set of the protocol",
                        set.name
                    )));
                }
            }
            if let Some(label) = &set.label {
                label.checked(&set.name)?;
                if let Some(target) = &label.target
                    && !names.contains(target)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{} labels its objects by a relation to {target}, which is not an object set of the protocol",
                        set.name
                    )));
                }
            }
            if let Some(color) = &set.track_by {
                color.checked(&format!("the tracks of {}", set.name))?;
                if set.track.is_none() {
                    return Err(ProtocolError::Invalid(format!(
                        "{} carries track colors without tracking",
                        set.name
                    )));
                }
            }
            match (self.kind_of(&set.name), &set.track) {
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
            slugs.push(key);
        }
        let measured = self.sets.iter().filter(|set| set.measure.is_some()).count();
        if measured > 1 {
            return Err(ProtocolError::Invalid(
                "a protocol measures one object set".to_string(),
            ));
        }
        let mut relations = Vec::new();
        for relation in &self.relations {
            validate(&relation.source, "a relation source")?;
            validate(&relation.target, "a relation target")?;
            if !names.contains(&relation.source) {
                return Err(ProtocolError::Invalid(format!(
                    "the relation source {} is not an object set of the protocol",
                    relation.source
                )));
            }
            if !names.contains(&relation.target) {
                return Err(ProtocolError::Invalid(format!(
                    "the relation target {} is not an object set of the protocol",
                    relation.target
                )));
            }
            if relation.source == relation.target && relation.shift == 0 {
                return Err(ProtocolError::Invalid(format!(
                    "{} cannot relate to itself without shifting in time",
                    relation.source
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
        for set in &self.sets {
            for color in [set.color_by.as_ref(), set.track_by.as_ref()]
                .into_iter()
                .flatten()
            {
                let Some(target) = &color.target else {
                    continue;
                };
                if !relations.contains(&(&set.name, target, color.shift)) {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is colored by a relation to {target} at the offset {} that the protocol does not measure",
                        set.name, color.shift
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
            if !relations.contains(&(&set.name, target, label.shift)) {
                return Err(ProtocolError::Invalid(format!(
                    "{} labels its objects by a relation to {target} at the offset {} that the protocol does not measure",
                    set.name, label.shift
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
                if !names.contains(mask) {
                    return Err(ProtocolError::Invalid(format!(
                        "the colocalization mask {mask} is not an object set of the protocol"
                    )));
                }
            }
        }
        if let Some(view) = &self.view {
            view.checked()?;
        }
        if let Some(snapshot) = self.snapshot {
            snapshot.checked()?;
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
        match source {
            SourceSpec::Surfaces { seed_source, .. } => {
                if let Some(source) = seed_source
                    && !earlier.contains(source)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "{set} seeds from {source}, which does not run before it"
                    )));
                }
                Ok(())
            }
            SourceSpec::Spots { .. } | SourceSpec::Filaments { .. } => Ok(()),
            SourceSpec::Filtered { source, .. } | SourceSpec::Copy { source } => {
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
