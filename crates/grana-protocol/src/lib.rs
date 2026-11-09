mod capture;
mod check;
mod derived;
mod present;
mod spec;
mod stage;
mod step;

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use spec::{
    AnchorSpec, AnnotationSpec, CameraSpec, ChannelSpec, ClassifierSpec, ColocPairSpec,
    ColocSettingsSpec, ColocSpec, ColorSpec, CurationSpec, DerivedSpec, DriftSpec, EditSpec,
    ExportSpec, FilterSpec, HiddenSpec, KeySpec, LassoSpec, MaskFilterSpec, MaskSpec, MeasureSpec,
    MeasuredSpec, MetricSpec, NodeSpec, PartSpec, RegionSpec, RelationSpec, RemovalSpec,
    RenderSpec, RestoreSpec, SectionSpec, SetSpec, ShadingSpec, SnapshotSpec, SortSpec, SourceSpec,
    StopSpec, SurfaceEditSpec, TimelineSpec, TrackSpec, TransferSpec, ViewSpec, WindowSpec,
};
pub use stage::Staged;
pub use step::Step;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region: Option<RegionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    render: Option<RenderSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channels: Vec<ChannelSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    derived: Vec<DerivedSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drift: Option<DriftSpec>,
    #[serde(default, skip_serializing_if = "spec::is_f32_zero")]
    mask_radius: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mask_filter: Option<MaskFilterSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    annotations: Vec<AnnotationSpec>,
    sets: Vec<SetSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relations: Vec<RelationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coloc: Option<ColocSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<ViewSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<SnapshotSpec>,
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
    pub fn region(&self) -> Option<RegionSpec> {
        self.region
    }

    pub fn render(&self) -> Option<RenderSpec> {
        self.render
    }

    pub fn channels(&self) -> &[ChannelSpec] {
        &self.channels
    }

    pub fn derived(&self) -> &[DerivedSpec] {
        &self.derived
    }

    pub fn drift(&self) -> Option<&DriftSpec> {
        self.drift.as_ref()
    }

    pub fn mask_radius(&self) -> f32 {
        self.mask_radius
    }

    pub fn mask_filter(&self) -> Option<&MaskFilterSpec> {
        self.mask_filter.as_ref()
    }

    pub fn annotations(&self) -> &[AnnotationSpec] {
        &self.annotations
    }

    pub fn sets(&self) -> &[SetSpec] {
        &self.sets
    }

    pub fn name_of(&self, key: &str) -> Option<&str> {
        self.sets
            .iter()
            .find(|set| set.key == key)
            .map(|set| set.name.as_str())
    }

    pub fn relations(&self) -> &[RelationSpec] {
        &self.relations
    }

    pub fn coloc(&self) -> Option<&ColocSpec> {
        self.coloc.as_ref()
    }

    pub fn view(&self) -> Option<&ViewSpec> {
        self.view.as_ref()
    }

    pub fn snapshot(&self) -> Option<SnapshotSpec> {
        self.snapshot
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
        let mut canonical = self.clone();
        for set in &mut canonical.sets {
            set.name = set.key.clone();
        }
        let text = serde_json::to_vec(&canonical).expect("a protocol always serializes");
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
                let mut text = match (&set.source, self.channel_of(&set.key)) {
                    (SourceSpec::Labels { label }, _) => {
                        format!("{} from the label image {label}", set.name)
                    }
                    (SourceSpec::Tracks { file }, _) => {
                        format!("{} from the track file {file}", set.name)
                    }
                    (_, Some(channel)) => format!("{} on {channel}", set.name),
                    (_, None) => format!("{} on an unknown channel", set.name),
                };
                match &set.source {
                    SourceSpec::Filtered { source, .. } => {
                        text = format!("{text} · filtered from {source}");
                    }
                    SourceSpec::Copy { source } => {
                        text = format!("{text} · copy of {source}");
                    }
                    SourceSpec::Edited { source, edits } => {
                        text = format!("{text} · {} surface edits of {source}", edits.len());
                    }
                    SourceSpec::Merged { parts } => {
                        let names = parts
                            .iter()
                            .map(|part| part.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" + ");
                        text = format!("{text} · merged from {names}");
                    }
                    SourceSpec::Surfaces {
                        seed_source: Some(source),
                        ..
                    } => {
                        text = format!("{text} · seeded from {source}");
                    }
                    SourceSpec::Spots { .. }
                    | SourceSpec::Surfaces { .. }
                    | SourceSpec::Filaments { .. }
                    | SourceSpec::Segments { .. }
                    | SourceSpec::Labels { .. }
                    | SourceSpec::Tracks { .. } => {}
                }
                if let Some(measure) = &set.measure {
                    let group = measure.group();
                    if group != grana_measure::Group::Object {
                        text = format!("{text} ({})", group.key());
                    }
                    let filters = measure.filters.len();
                    if filters > 0 {
                        text = format!(
                            "{text} · {filters} filter{}",
                            if filters == 1 { "" } else { "s" }
                        );
                    }
                }
                if let Some(classifier) = &set.classifier {
                    text = format!(
                        "{text} · classifier on {} measurements",
                        classifier.features.len()
                    );
                }
                if let Some(mask) = &set.mask {
                    text = format!("{text} within {}", self.name_of(mask).unwrap_or(mask));
                }
                if let Some(curation) = &set.curation {
                    let removed = curation
                        .removed
                        .iter()
                        .map(|removal| removal.anchors.len())
                        .sum::<usize>();
                    if removed > 0 {
                        text = format!("{text} · {removed} removed by hand");
                    }
                    let hidden = curation
                        .hidden
                        .iter()
                        .map(|hidden| hidden.anchors.len())
                        .sum::<usize>();
                    if hidden > 0 {
                        text = format!("{text} · {hidden} hidden by hand");
                    }
                    if !curation.edits.is_empty() {
                        text = format!("{text} · {} track edits", curation.edits.len());
                    }
                }
                text
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut summary = match &self.region {
            Some(region) => {
                let size = region.size();
                format!(
                    "{sets} · region {:.1}×{:.1}×{:.1} µm",
                    size.x, size.y, size.z
                )
            }
            None => sets,
        };
        if let Some(drift) = &self.drift {
            summary.push_str(&format!(
                " · aligned on {} to frame {}",
                drift.channel, drift.reference
            ));
        }
        if self.mask_radius > 0.0 {
            summary.push_str(&format!(" · mask grows {:.2} µm", self.mask_radius));
        }
        if let Some(filter) = &self.mask_filter {
            summary.push_str(&format!(
                " · mask keeps {} {}–{} of {}",
                filter.field,
                filter.minimum,
                filter.maximum,
                self.name_of(&filter.target).unwrap_or(&filter.target)
            ));
        }
        if !self.derived.is_empty() {
            summary.push_str(&format!(" · {} derived channels", self.derived.len()));
        }
        if !self.annotations.is_empty() {
            summary.push_str(&format!(" · {} annotations", self.annotations.len()));
        }
        for relation in &self.relations {
            summary.push_str(&format!(
                " · relates {} to {}",
                self.name_of(&relation.source).unwrap_or(&relation.source),
                match relation.shift {
                    0 => self
                        .name_of(&relation.target)
                        .unwrap_or(&relation.target)
                        .to_string(),
                    shift => format!(
                        "{} (t{shift:+})",
                        self.name_of(&relation.target).unwrap_or(&relation.target)
                    ),
                }
            ));
        }
        if let Some(coloc) = &self.coloc {
            let pairs = coloc
                .pairs
                .iter()
                .map(|pair| format!("{} with {}", pair.a, pair.b))
                .collect::<Vec<_>>()
                .join(", ");
            let mask = coloc.mask.as_ref().map_or_else(String::new, |mask| {
                format!(" within {}", self.name_of(mask).unwrap_or(mask))
            });
            summary.push_str(&format!(
                " · colocalizes {pairs}{mask} ({} permutations)",
                coloc.settings.permutations
            ));
        }
        if let Some(timeline) = self.view.as_ref().and_then(|view| view.timeline.as_ref()) {
            summary.push_str(&format!(
                " · {} fps over {:.1} s",
                timeline.fps, timeline.duration
            ));
        }
        if let Some(snapshot) = self.snapshot {
            summary.push_str(&format!(
                " · snapshot {}×{}",
                snapshot.width, snapshot.height
            ));
        }
        summary
    }
}
