use std::path::Path;

use glam::Vec3;
use grana_analysis::{FilamentParams, SpotParams, SurfaceFilter, SurfaceParams, TrackParams};
use grana_measure::{
    Aggregate, COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX, ColocSettings, Filter, Group,
};
use grana_scene::{Measure, ObjectKind, ObjectParams, Scene, slug};
use grana_volume::Region;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region: Option<RegionSpec>,
    sets: Vec<SetSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relations: Vec<RelationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coloc: Option<ColocSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationSpec {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    pub low: [f32; 3],
    pub high: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    pub name: String,
    pub channel: String,
    pub color: [f32; 3],
    pub detection: DetectionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSpec>,
    #[serde(default)]
    pub measure: MeasureSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasureSpec {
    pub group: String,
    pub aggregate: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    pub column: String,
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DetectionSpec {
    Spots {
        diameter: f32,
        contrast: f32,
    },
    Surfaces {
        threshold: f32,
        grain: f32,
        seed: f32,
        min_volume: f32,
        opening: f32,
        closing: f32,
        min_sphericity: f32,
        min_mean_intensity: f32,
    },
    Filaments {
        threshold: f32,
        min_length: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSpec {
    pub max_step: f32,
    pub max_gap: u32,
    pub division: bool,
    pub division_range: f32,
    pub thickness: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocSpec {
    pub settings: ColocSettingsSpec,
    pub pairs: Vec<ColocPairSpec>,
    #[serde(deserialize_with = "mask_field")]
    pub mask: Option<String>,
}

impl ColocSpec {
    pub fn settings(&self) -> ColocSettings {
        ColocSettings::new(self.settings.permutations, self.settings.block)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocSettingsSpec {
    pub permutations: u32,
    pub block: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocPairSpec {
    pub a: String,
    pub b: String,
    pub thresholds: [f32; 2],
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("the protocol is not valid json: {0}")]
    Json(String),
    #[error("the protocol is not valid: {0}")]
    Invalid(String),
    #[error("failed to read the protocol at {path}: {detail}")]
    Read { path: String, detail: String },
    #[error("failed to write the protocol at {path}: {detail}")]
    Write { path: String, detail: String },
}

impl Protocol {
    pub fn new(
        region: Option<RegionSpec>,
        sets: Vec<SetSpec>,
        relations: Vec<RelationSpec>,
        coloc: Option<ColocSpec>,
    ) -> Result<Self, ProtocolError> {
        Self {
            region,
            sets,
            relations,
            coloc,
        }
        .checked()
    }

    pub fn region(&self) -> Option<RegionSpec> {
        self.region
    }

    pub fn sets(&self) -> &[SetSpec] {
        &self.sets
    }

    pub fn relations(&self) -> &[RelationSpec] {
        &self.relations
    }

    pub fn coloc(&self) -> Option<&ColocSpec> {
        self.coloc.as_ref()
    }

    pub fn capture(scene: &Scene, measure: &Measure) -> Result<Self, ProtocolError> {
        let region = RegionSpec::of(scene.region, scene.dataset.world_size());
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let mask = scene
            .mask
            .and_then(|id| scene.objects.get(id))
            .map(|set| set.id());
        let mut sets = scene
            .objects
            .iter()
            .map(|set| {
                let detection = match set.params() {
                    ObjectParams::Spots(params) => DetectionSpec::Spots {
                        diameter: params.diameter(),
                        contrast: params.threshold(),
                    },
                    ObjectParams::Surfaces(params) => DetectionSpec::Surfaces {
                        threshold: params.threshold(),
                        grain: params.grain(),
                        seed: params.seed(),
                        min_volume: params.min_volume(),
                        opening: params.opening(),
                        closing: params.closing(),
                        min_sphericity: params.min_sphericity(),
                        min_mean_intensity: params.min_mean_intensity(),
                    },
                    ObjectParams::Filaments(params) => DetectionSpec::Filaments {
                        threshold: params.threshold(),
                        min_length: params.min_length(),
                    },
                };
                let track = set.track_style().map(|style| TrackSpec {
                    max_step: style.params().max_step(),
                    max_gap: style.params().max_gap(),
                    division: style.params().division(),
                    division_range: style.params().division_range(),
                    thickness: style.thickness(),
                });
                let measure = if measure.set() == Some(set.id()) {
                    MeasureSpec::of(measure)
                } else {
                    MeasureSpec::default()
                };
                SetSpec {
                    name: set.name().to_string(),
                    channel: channels[set.channel()].clone(),
                    color: set.color(),
                    detection,
                    mask: None,
                    track,
                    measure,
                }
            })
            .collect::<Vec<_>>();
        if let Some(mask) = mask {
            let position = scene
                .objects
                .iter()
                .position(|set| set.id() == mask)
                .expect("the analysis mask names a set of the scene");
            let name = sets[position].name.clone();
            sets.rotate_left(position);
            for set in &mut sets[1..] {
                set.mask = Some(name.clone());
            }
        }
        let relations = scene
            .objects
            .iter()
            .flat_map(|set| {
                set.relations().iter().filter_map(|relation| {
                    let target = scene.objects.get(relation.target())?;
                    Some(RelationSpec {
                        source: set.name().to_string(),
                        target: target.name().to_string(),
                    })
                })
            })
            .collect::<Vec<_>>();
        let coloc = if scene.coloc.results().is_empty() {
            None
        } else {
            let pairs = scene
                .coloc
                .results()
                .iter()
                .map(|result| {
                    let pair = result.pair();
                    ColocPairSpec {
                        a: channels[pair.a()].clone(),
                        b: channels[pair.b()].clone(),
                        thresholds: [result.thresholds().a(), result.thresholds().b()],
                    }
                })
                .collect::<Vec<_>>();
            let mask = scene
                .mask
                .and_then(|id| scene.objects.get(id))
                .map(|set| set.name().to_string());
            let settings = scene.coloc.settings();
            Some(ColocSpec {
                settings: ColocSettingsSpec {
                    permutations: settings.permutations(),
                    block: settings.block(),
                },
                pairs,
                mask,
            })
        };
        Self::new(region, sets, relations, coloc)
    }

    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string_pretty(self).map_err(|error| ProtocolError::Json(error.to_string()))
    }

    pub fn from_json(text: &str) -> Result<Self, ProtocolError> {
        let protocol = serde_json::from_str::<Self>(text)
            .map_err(|error| ProtocolError::Json(error.to_string()))?;
        protocol.checked()
    }

    pub fn read(path: &Path) -> Result<Self, ProtocolError> {
        let text = std::fs::read_to_string(path).map_err(|error| ProtocolError::Read {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
        Self::from_json(&text)
    }

    pub fn write(&self, path: &Path) -> Result<(), ProtocolError> {
        std::fs::write(path, self.to_json()?).map_err(|error| ProtocolError::Write {
            path: path.display().to_string(),
            detail: error.to_string(),
        })
    }

    pub fn digest(&self) -> u64 {
        let text = serde_json::to_vec(self).expect("a protocol always serializes");
        text.into_iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }

    pub fn summary(&self) -> String {
        let sets = self
            .sets
            .iter()
            .map(|set| {
                let mut text = format!("{} on {}", set.name, set.channel);
                let group = set.measure.group();
                if group != Group::Object {
                    text = format!("{text} ({})", group.key());
                }
                let filters = set.measure.filters.len();
                if filters > 0 {
                    text = format!(
                        "{text} · {filters} filter{}",
                        if filters == 1 { "" } else { "s" }
                    );
                }
                if let Some(mask) = &set.mask {
                    text = format!("{text} within {mask}");
                }
                text
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut summary = match &self.region {
            Some(region) => format!("{sets} · region {:.0}%", region.percent()),
            None => sets,
        };
        for relation in &self.relations {
            summary.push_str(&format!(
                " · relates {} to {}",
                relation.source, relation.target
            ));
        }
        if let Some(coloc) = &self.coloc {
            let pairs = coloc
                .pairs
                .iter()
                .map(|pair| format!("{} with {}", pair.a, pair.b))
                .collect::<Vec<_>>()
                .join(", ");
            let mask = coloc
                .mask
                .as_ref()
                .map_or_else(String::new, |mask| format!(" within {mask}"));
            summary.push_str(&format!(
                " · colocalizes {pairs}{mask} ({} permutations)",
                coloc.settings.permutations
            ));
        }
        summary
    }

    fn checked(self) -> Result<Self, ProtocolError> {
        if self.sets.is_empty() {
            return Err(ProtocolError::Invalid(
                "a protocol names at least one object set".to_string(),
            ));
        }
        if let Some(region) = &self.region {
            region.checked()?;
        }
        let mut names = Vec::new();
        let mut slugs = Vec::new();
        for set in &self.sets {
            validate(&set.name, "a set name")?;
            validate(&set.channel, "a channel name")?;
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
            names.push(set.name.clone());
            slugs.push(key);
            if let Some(mask) = &set.mask {
                validate(mask, "an analysis mask")?;
                if !names[..names.len() - 1].contains(mask) {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is masked by an object set that does not run before it",
                        set.name
                    )));
                }
                if mask == &set.name {
                    return Err(ProtocolError::Invalid(format!(
                        "{} cannot be masked by itself",
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
            set.detection.checked(&set.name)?;
            match (set.detection.kind(), &set.track) {
                (ObjectKind::Filaments, Some(_)) => {
                    return Err(ProtocolError::Invalid(format!(
                        "{} is a filament set and cannot be tracked",
                        set.name
                    )));
                }
                (_, Some(track)) => track.checked(&set.name)?,
                (_, None) => {}
            }
            set.measure.checked(&set.name, set.track.is_some())?;
        }
        let mut pairs = Vec::new();
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
            if relation.source == relation.target {
                return Err(ProtocolError::Invalid(format!(
                    "{} cannot relate to itself",
                    relation.source
                )));
            }
            let pair = (&relation.source, &relation.target);
            if pairs.contains(&pair) {
                return Err(ProtocolError::Invalid(format!(
                    "{} relates to {} twice",
                    relation.source, relation.target
                )));
            }
            pairs.push(pair);
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
        Ok(self)
    }
}

impl RegionSpec {
    pub fn of(region: Region, world: Vec3) -> Option<Self> {
        if !region.enabled() {
            return None;
        }
        let half = 0.5 * world;
        let axis =
            |values: Vec3| [0, 1, 2].map(|index| (values[index] + half[index]) / world[index]);
        Some(Self {
            low: axis(region.min()),
            high: axis(region.max()),
        })
    }

    pub fn region(self, world: Vec3) -> Region {
        let half = 0.5 * world;
        let axis = |values: [f32; 3]| {
            Vec3::new(
                values[0] * world.x - half.x,
                values[1] * world.y - half.y,
                values[2] * world.z - half.z,
            )
        };
        Region::new(axis(self.low), axis(self.high))
    }

    pub fn percent(self) -> f32 {
        let span = (0..3).fold(1.0f32, |product, axis| {
            product * (self.high[axis] - self.low[axis]).max(0.0)
        });
        span * 100.0
    }

    fn checked(self) -> Result<(), ProtocolError> {
        for axis in 0..3 {
            let low = self.low[axis];
            let high = self.high[axis];
            if !(low.is_finite() && high.is_finite()) {
                return Err(ProtocolError::Invalid("a region box is finite".to_string()));
            }
            if !(0.0..=1.0).contains(&low) || !(0.0..=1.0).contains(&high) {
                return Err(ProtocolError::Invalid(
                    "a region box lies inside the dataset".to_string(),
                ));
            }
            if low >= high {
                return Err(ProtocolError::Invalid(
                    "a region box holds a positive extent on every axis".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl SetSpec {
    pub fn kind(&self) -> ObjectKind {
        self.detection.kind()
    }

    pub fn params(&self) -> ObjectParams {
        self.detection.params()
    }

    pub fn mask(&self) -> Option<&str> {
        self.mask.as_deref()
    }

    pub fn track_params(&self) -> Option<TrackParams> {
        self.track.map(|track| {
            TrackParams::new(
                track.max_step,
                track.max_gap,
                track.division,
                track.division_range,
            )
        })
    }

    pub fn group(&self) -> Group {
        self.measure.group()
    }

    pub fn aggregate(&self) -> Aggregate {
        self.measure.aggregate()
    }

    pub fn filters(&self, table: &grana_measure::Table) -> Result<Vec<Filter>, String> {
        self.measure
            .filters
            .iter()
            .map(|filter| {
                let column = table.column(&filter.column).ok_or_else(|| {
                    format!(
                        "the table of {} has no column named {}",
                        self.name, filter.column
                    )
                })?;
                Ok(Filter::new(column, filter.minimum, filter.maximum))
            })
            .collect()
    }
}

impl MeasureSpec {
    pub fn of(measure: &Measure) -> Self {
        let group = measure.group();
        let aggregate = if group == Group::Object {
            Aggregate::Mean
        } else {
            measure.aggregate()
        };
        let filters = match measure.table() {
            Some(table) => measure
                .filters()
                .iter()
                .map(|filter| {
                    let column = table
                        .columns()
                        .get(filter.column())
                        .expect("a measure filter names a column of its table");
                    FilterSpec {
                        column: column.key().to_string(),
                        minimum: filter.minimum(),
                        maximum: filter.maximum(),
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        Self {
            group: group.key().to_string(),
            aggregate: aggregate.key().to_string(),
            filters,
        }
    }

    pub fn group(&self) -> Group {
        Group::parse(&self.group).expect("a measure group names a table grouping")
    }

    pub fn aggregate(&self) -> Aggregate {
        Aggregate::parse(&self.aggregate).expect("a measure aggregate names a reduction")
    }

    fn checked(&self, set: &str, tracked: bool) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        let group = Group::parse(&self.group).ok_or_else(|| {
            ProtocolError::Invalid(format!("the set {set} names an unknown grouping"))
        })?;
        let aggregate = Aggregate::parse(&self.aggregate).ok_or_else(|| {
            ProtocolError::Invalid(format!("the set {set} names an unknown aggregate"))
        })?;
        if group == Group::Object && aggregate != Aggregate::Mean {
            return invalid("measures objects without an aggregate".to_string());
        }
        if group == Group::Track && !tracked {
            return invalid("groups by tracks without tracking".to_string());
        }
        for filter in &self.filters {
            if filter.column.trim().is_empty() {
                return invalid("filters an unnamed column".to_string());
            }
            if !(filter.minimum.is_finite() && filter.maximum.is_finite()) {
                return invalid("filters a range that is not finite".to_string());
            }
            if filter.minimum > filter.maximum {
                return invalid("filters a range that is not ordered".to_string());
            }
        }
        Ok(())
    }
}

impl Default for MeasureSpec {
    fn default() -> Self {
        Self {
            group: Group::Object.key().to_string(),
            aggregate: Aggregate::Mean.key().to_string(),
            filters: Vec::new(),
        }
    }
}

impl DetectionSpec {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Spots { .. } => ObjectKind::Spots,
            Self::Surfaces { .. } => ObjectKind::Surfaces,
            Self::Filaments { .. } => ObjectKind::Filaments,
        }
    }

    pub fn params(&self) -> ObjectParams {
        match *self {
            Self::Spots { diameter, contrast } => {
                ObjectParams::Spots(SpotParams::new(diameter, contrast))
            }
            Self::Surfaces {
                threshold,
                grain,
                seed,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
            } => ObjectParams::Surfaces(SurfaceParams::new(
                threshold,
                grain,
                seed,
                min_volume,
                SurfaceFilter::new(opening, closing, min_sphericity, min_mean_intensity),
            )),
            Self::Filaments {
                threshold,
                min_length,
            } => ObjectParams::Filaments(FilamentParams::new(threshold, min_length)),
        }
    }

    fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        match *self {
            Self::Spots { diameter, contrast } => {
                if !(diameter.is_finite() && diameter > 0.0) {
                    return invalid("needs a positive spot diameter".to_string());
                }
                if !(0.0..=1.0).contains(&contrast) {
                    return invalid("needs a unit contrast threshold".to_string());
                }
            }
            Self::Surfaces {
                threshold,
                grain,
                seed,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
            } => {
                if !(0.0..=1.0).contains(&threshold) {
                    return invalid("needs a unit surface threshold".to_string());
                }
                if !(grain.is_finite() && grain >= 0.0) {
                    return invalid("needs a non-negative surface grain".to_string());
                }
                if !(seed.is_finite() && seed >= 0.0) {
                    return invalid("needs a non-negative surface seed".to_string());
                }
                if !(min_volume.is_finite() && min_volume >= 0.0) {
                    return invalid("needs a non-negative minimum volume".to_string());
                }
                if !(opening.is_finite() && opening >= 0.0) {
                    return invalid("needs a non-negative surface opening radius".to_string());
                }
                if !(closing.is_finite() && closing >= 0.0) {
                    return invalid("needs a non-negative surface closing radius".to_string());
                }
                if !(0.0..=1.0).contains(&min_sphericity) {
                    return invalid("needs a unit sphericity filter".to_string());
                }
                if !(0.0..=1.0).contains(&min_mean_intensity) {
                    return invalid("needs a unit intensity filter".to_string());
                }
            }
            Self::Filaments {
                threshold,
                min_length,
            } => {
                if !(0.0..=1.0).contains(&threshold) {
                    return invalid("needs a unit filament threshold".to_string());
                }
                if !(min_length.is_finite() && min_length > 0.0) {
                    return invalid("needs a positive minimum length".to_string());
                }
            }
        }
        Ok(())
    }
}

impl TrackSpec {
    fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the track of {set} {detail}"
            )))
        };
        if !(self.max_step.is_finite() && self.max_step > 0.0) {
            return invalid("needs a positive step".to_string());
        }
        if !(self.division_range.is_finite() && self.division_range > 0.0) {
            return invalid("needs a positive division range".to_string());
        }
        if self.division && self.division_range < self.max_step {
            return invalid("needs a division range that covers the step".to_string());
        }
        if !(self.thickness.is_finite() && self.thickness > 0.0) {
            return invalid("needs a positive tube thickness".to_string());
        }
        Ok(())
    }
}

fn validate(value: &str, what: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::Invalid(format!("{what} is empty")));
    }
    Ok(())
}

fn mask_field<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}
