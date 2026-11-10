use grana_object::Algorithm;
use grana_scene::{Scene, Trace};
use grana_volume::Fingerprint;
use serde::{Deserialize, Serialize};

use crate::ProtocolError;

const FINGERPRINT_HEX: usize = 64;

const TRACE_PREFIX: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginSpec {
    pub software: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    pub algorithms: Vec<AlgorithmSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<ExpectedSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlgorithmSpec {
    pub name: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSpec {
    pub set: String,
    pub algorithm: String,
    pub version: u32,
    pub objects: usize,
}

impl OriginSpec {
    pub fn of(scene: &Scene, fingerprint: Option<Fingerprint>) -> Self {
        let algorithms = Algorithm::ALL
            .iter()
            .map(|algorithm| AlgorithmSpec {
                name: algorithm.name().to_string(),
                version: algorithm.version(),
            })
            .collect();
        let sets = scene
            .objects
            .iter()
            .filter(|set| !set.provenance().is_attached())
            .map(|set| {
                let trace = Trace::of(set, fingerprint);
                ExpectedSpec {
                    set: set.key().as_str().to_string(),
                    algorithm: trace.algorithm().name().to_string(),
                    version: trace.version(),
                    objects: trace.objects(),
                }
            })
            .collect();
        Self {
            software: format!("grana {}", env!("CARGO_PKG_VERSION")),
            fingerprint: fingerprint.map(Fingerprint::hex),
            algorithms,
            sets,
        }
    }

    pub fn software(&self) -> &str {
        &self.software
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }

    pub fn algorithms(&self) -> &[AlgorithmSpec] {
        &self.algorithms
    }

    pub fn sets(&self) -> &[ExpectedSpec] {
        &self.sets
    }

    pub fn verify(&self, scene: &Scene, fingerprint: Option<Fingerprint>) -> Vec<String> {
        let mut drift = Vec::new();
        if let Some(recorded) = &self.fingerprint {
            match fingerprint {
                Some(actual) if actual.hex() != *recorded => drift.push(format!(
                    "the replay reads another dataset ({} against {})",
                    short(recorded),
                    short(&actual.hex())
                )),
                None => drift.push("the replay cannot fingerprint its dataset".to_string()),
                Some(_) => {}
            }
        }
        for spec in &self.algorithms {
            let Some(algorithm) = Algorithm::from_name(&spec.name) else {
                drift.push(format!(
                    "the recorded algorithm {} is unknown to this build",
                    spec.name
                ));
                continue;
            };
            if algorithm.version() != spec.version {
                drift.push(format!(
                    "{} produced the objects at v{}, this build runs v{}",
                    algorithm.title(),
                    spec.version,
                    algorithm.version()
                ));
            }
        }
        for expected in &self.sets {
            let found = scene
                .objects
                .iter()
                .find(|set| set.key().as_str() == expected.set);
            let Some(set) = found else {
                drift.push(format!("{} is missing from the replay", expected.set));
                continue;
            };
            let trace = Trace::of(set, fingerprint);
            if trace.algorithm().name() != expected.algorithm || trace.version() != expected.version
            {
                drift.push(format!(
                    "{}: {} v{}, recorded {} v{}",
                    expected.set,
                    trace.algorithm().title(),
                    trace.version(),
                    expected.algorithm,
                    expected.version
                ));
            }
            if trace.objects() != expected.objects {
                drift.push(format!(
                    "{}: {} objects, recorded {}",
                    expected.set,
                    trace.objects(),
                    expected.objects
                ));
            }
        }
        drift
    }

    pub(crate) fn checked(&self, sets: &[String]) -> Result<(), ProtocolError> {
        if self.software.trim().is_empty() {
            return Err(ProtocolError::Invalid(
                "the protocol origin names its software".to_string(),
            ));
        }
        if let Some(fingerprint) = &self.fingerprint
            && (fingerprint.len() != FINGERPRINT_HEX
                || Fingerprint::from_hex(fingerprint).is_none())
        {
            return Err(ProtocolError::Invalid(
                "the protocol origin records a sha256 fingerprint".to_string(),
            ));
        }
        let mut names = Vec::new();
        for algorithm in &self.algorithms {
            if algorithm.name.trim().is_empty() || names.contains(&algorithm.name) {
                return Err(ProtocolError::Invalid(format!(
                    "the origin lists the algorithm {} twice or without a name",
                    algorithm.name
                )));
            }
            if algorithm.version == 0 {
                return Err(ProtocolError::Invalid(format!(
                    "the algorithm {} starts at version one",
                    algorithm.name
                )));
            }
            names.push(algorithm.name.clone());
        }
        let mut keys = Vec::new();
        for expected in &self.sets {
            if !sets.contains(&expected.set) {
                return Err(ProtocolError::Invalid(format!(
                    "the origin records {} objects for the unknown set {}",
                    expected.objects, expected.set
                )));
            }
            if expected.algorithm.trim().is_empty() || expected.version == 0 {
                return Err(ProtocolError::Invalid(format!(
                    "the origin names the algorithm of {} with a version",
                    expected.set
                )));
            }
            if keys.contains(&expected.set) {
                return Err(ProtocolError::Invalid(format!(
                    "the origin records {} twice",
                    expected.set
                )));
            }
            keys.push(expected.set.clone());
        }
        Ok(())
    }
}

fn short(fingerprint: &str) -> &str {
    fingerprint.get(..TRACE_PREFIX).unwrap_or(fingerprint)
}
