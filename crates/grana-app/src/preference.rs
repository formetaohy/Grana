use std::path::PathBuf;

use grana_ui::Appearance;
use serde::{Deserialize, Serialize};

const FILE: &str = "preferences.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredAppearance {
    System,
    Light,
    Dark,
}

impl From<Appearance> for StoredAppearance {
    fn from(appearance: Appearance) -> Self {
        match appearance {
            Appearance::System => Self::System,
            Appearance::Light => Self::Light,
            Appearance::Dark => Self::Dark,
        }
    }
}

impl From<StoredAppearance> for Appearance {
    fn from(appearance: StoredAppearance) -> Self {
        match appearance {
            StoredAppearance::System => Self::System,
            StoredAppearance::Light => Self::Light,
            StoredAppearance::Dark => Self::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    appearance: StoredAppearance,
    contrast: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preference {
    pub appearance: Appearance,
    pub contrast: bool,
}

impl Default for Preference {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            contrast: false,
        }
    }
}

impl Preference {
    pub fn load() -> Self {
        let Some(path) = storage() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let text = text.trim_start_matches('\u{feff}');
        let Ok(stored) = serde_json::from_str::<Stored>(text) else {
            return Self::default();
        };
        Self {
            appearance: stored.appearance.into(),
            contrast: stored.contrast,
        }
    }

    pub fn save(self) -> Result<(), String> {
        let Some(path) = storage() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let stored = Stored {
            appearance: self.appearance.into(),
            contrast: self.contrast,
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| format!("failed to encode the preferences: {error}"))?;
        grana_file::write(&path, &bytes)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }
}

fn storage() -> Option<PathBuf> {
    grana_dir::data().map(|root| root.join(FILE))
}
