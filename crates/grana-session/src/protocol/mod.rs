mod capture;
mod check;
mod present;
mod spec;
mod stage;
mod step;

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use spec::{
    AxisSpec, CameraSpec, ChannelSpec, ColocPairSpec, ColocSettingsSpec, ColocSpec, ColorSpec,
    DetectionSpec, FilterSpec, KeySpec, MeasureSpec, RegionSpec, RelationSpec, RenderSpec,
    SectionSpec, SetSpec, ShadingSpec, SortSpec, StopSpec, TimelineSpec, TrackSpec, TransferSpec,
    ViewSpec, WindowSpec,
};
pub use stage::Staged;
pub(crate) use step::Step;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region: Option<RegionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    render: Option<RenderSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channels: Vec<ChannelSpec>,
    sets: Vec<SetSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relations: Vec<RelationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coloc: Option<ColocSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<ViewSpec>,
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
    pub(crate) fn build(
        region: Option<RegionSpec>,
        render: Option<RenderSpec>,
        channels: Vec<ChannelSpec>,
        sets: Vec<SetSpec>,
        relations: Vec<RelationSpec>,
        coloc: Option<ColocSpec>,
        view: Option<ViewSpec>,
    ) -> Result<Self, ProtocolError> {
        Self {
            region,
            render,
            channels,
            sets,
            relations,
            coloc,
            view,
        }
        .checked()
    }

    pub fn region(&self) -> Option<RegionSpec> {
        self.region
    }

    pub fn render(&self) -> Option<RenderSpec> {
        self.render
    }

    pub fn channels(&self) -> &[ChannelSpec] {
        &self.channels
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

    pub fn view(&self) -> Option<&ViewSpec> {
        self.view.as_ref()
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
        if let Some(timeline) = self.view.as_ref().and_then(|view| view.timeline.as_ref()) {
            summary.push_str(&format!(
                " · {} fps over {:.1} s",
                timeline.fps, timeline.duration
            ));
        }
        summary
    }
}
