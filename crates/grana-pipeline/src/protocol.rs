use std::path::Path;

use grana_analysis::{FilamentParams, SpotParams, SurfaceParams, TrackParams};
use grana_scene::{ObjectKind, ObjectParams, Scene};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::slug::slug;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    sets: Vec<SetSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coloc: Option<ColocSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    pub name: String,
    pub channel: String,
    pub color: [f32; 3],
    pub detection: DetectionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DetectionSpec {
    Spots { diameter: f32, contrast: f32 },
    Surfaces { threshold: f32, min_volume: f32 },
    Filaments { threshold: f32, min_length: f32 },
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
    pub fn new(sets: Vec<SetSpec>, coloc: Option<ColocSpec>) -> Result<Self, ProtocolError> {
        Self { sets, coloc }.checked()
    }

    pub fn sets(&self) -> &[SetSpec] {
        &self.sets
    }

    pub fn coloc(&self) -> Option<&ColocSpec> {
        self.coloc.as_ref()
    }

    pub fn capture(scene: &Scene) -> Result<Self, ProtocolError> {
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let sets = scene
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
                        min_volume: params.min_volume(),
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
                SetSpec {
                    name: set.name().to_string(),
                    channel: channels[set.channel()].clone(),
                    color: set.color(),
                    detection,
                    track,
                }
            })
            .collect();
        let coloc = if scene.coloc.series().is_some() {
            scene.coloc.pair().map(|pair| ColocSpec {
                a: channels[pair.a()].clone(),
                b: channels[pair.b()].clone(),
                thresholds: [scene.coloc.thresholds().a(), scene.coloc.thresholds().b()],
            })
        } else {
            None
        };
        Self::new(sets, coloc)
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

    pub fn summary(&self) -> String {
        let sets = self
            .sets
            .iter()
            .map(|set| format!("{} on {}", set.name, set.channel))
            .collect::<Vec<_>>()
            .join(", ");
        match &self.coloc {
            Some(coloc) => format!("{sets} · colocalizes {} with {}", coloc.a, coloc.b),
            None => sets,
        }
    }

    fn checked(self) -> Result<Self, ProtocolError> {
        if self.sets.is_empty() {
            return Err(ProtocolError::Invalid(
                "a protocol names at least one object set".to_string(),
            ));
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
        }
        if let Some(coloc) = &self.coloc {
            validate(&coloc.a, "a channel name")?;
            validate(&coloc.b, "a channel name")?;
            if coloc.a == coloc.b {
                return Err(ProtocolError::Invalid(
                    "a colocalization pair names two channels".to_string(),
                ));
            }
            if coloc
                .thresholds
                .iter()
                .any(|value| !(0.0..=1.0).contains(value))
            {
                return Err(ProtocolError::Invalid(
                    "a colocalization threshold is a unit fraction".to_string(),
                ));
            }
        }
        Ok(self)
    }
}

impl SetSpec {
    pub fn kind(&self) -> ObjectKind {
        self.detection.kind()
    }

    pub fn params(&self) -> ObjectParams {
        self.detection.params()
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
                min_volume,
            } => ObjectParams::Surfaces(SurfaceParams::new(threshold, min_volume)),
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
                min_volume,
            } => {
                if !(0.0..=1.0).contains(&threshold) {
                    return invalid("needs a unit surface threshold".to_string());
                }
                if !(min_volume.is_finite() && min_volume >= 0.0) {
                    return invalid("needs a non-negative minimum volume".to_string());
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
