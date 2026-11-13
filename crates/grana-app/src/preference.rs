use std::path::PathBuf;

use grana_i18n::Language;
use serde::{Deserialize, Serialize};

const FILE: &str = "preferences.toml";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredLanguage {
    English,
    Chinese,
}

impl From<Language> for StoredLanguage {
    fn from(language: Language) -> Self {
        match language {
            Language::English => Self::English,
            Language::Chinese => Self::Chinese,
        }
    }
}

impl From<StoredLanguage> for Language {
    fn from(language: StoredLanguage) -> Self {
        match language {
            StoredLanguage::English => Self::English,
            StoredLanguage::Chinese => Self::Chinese,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    #[serde(default)]
    language: Option<StoredLanguage>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preference {
    pub language: Language,
}

impl Default for Preference {
    fn default() -> Self {
        Self {
            language: Language::system(),
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
        let Ok(stored) = toml::from_str::<Stored>(text) else {
            return Self::default();
        };
        Self {
            language: stored.language.map_or_else(Language::system, Into::into),
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
            language: Some(self.language.into()),
        };
        let text = toml::to_string_pretty(&stored)
            .map_err(|error| format!("failed to encode the preferences: {error}"))?;
        grana_file::write(&path, text.as_bytes())
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }
}

fn storage() -> Option<PathBuf> {
    grana_dir::data().map(|root| root.join(FILE))
}
