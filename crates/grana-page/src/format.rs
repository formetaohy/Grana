use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Svg,
    Pdf,
}

impl Format {
    pub fn of(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("svg") => Some(Self::Svg),
            Some("pdf") => Some(Self::Pdf),
            _ => None,
        }
    }

    pub fn error(path: &Path, subject: &str) -> String {
        format!("{subject} is saved as SVG or PDF, not `{}`", path.display())
    }
}
