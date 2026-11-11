#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    English,
    Chinese,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Chinese];
    pub const ENVIRONMENT: &str = "GRANA_LANGUAGE";

    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "中文",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        let primary = tag.trim().to_ascii_lowercase();
        let primary = primary.split(['-', '_']).next().unwrap_or_default();
        match primary {
            "en" => Some(Self::English),
            "zh" => Some(Self::Chinese),
            _ => None,
        }
    }

    pub fn from_environment() -> Option<Self> {
        std::env::var(Self::ENVIRONMENT)
            .ok()
            .as_deref()
            .and_then(Self::from_tag)
    }

    pub fn system() -> Self {
        sys_locale::get_locale()
            .as_deref()
            .and_then(Self::from_tag)
            .unwrap_or(Self::English)
    }
}
