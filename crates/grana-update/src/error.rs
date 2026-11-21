use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the update source `{location}` is unusable: {detail}")]
    Source { location: String, detail: String },
    #[error("the update manifest `{url}` is unusable: {detail}")]
    Manifest { url: String, detail: String },
    #[error("the update offer is unusable: {detail}")]
    Offer { detail: String },
    #[error("the update download failed: {detail}")]
    Network { detail: String },
    #[error("`{path}` does not match the update manifest")]
    Corrupt { path: String },
    #[error("`{path}` cannot be written: {detail}")]
    Storage { path: String, detail: String },
    #[error("the update was stopped")]
    Cancelled,
    #[error("the installer did not run: {detail}")]
    Installer { detail: String },
    #[error("the browser did not open: {detail}")]
    Browser { detail: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Offline,
    Corrupt,
    Storage,
    Installer,
    Other,
}

impl Error {
    pub fn failure(&self) -> Failure {
        match self {
            Self::Network { .. } => Failure::Offline,
            Self::Corrupt { .. } => Failure::Corrupt,
            Self::Storage { .. } => Failure::Storage,
            Self::Installer { .. } => Failure::Installer,
            Self::Source { .. } | Self::Manifest { .. } | Self::Offer { .. } => Failure::Other,
            Self::Browser { .. } | Self::Cancelled => Failure::Other,
        }
    }

    pub fn storage(path: impl AsRef<std::path::Path>, error: std::io::Error) -> Self {
        Self::Storage {
            path: path.as_ref().display().to_string(),
            detail: error.to_string(),
        }
    }
}

impl From<grana_net::Error> for Error {
    fn from(error: grana_net::Error) -> Self {
        match error {
            grana_net::Error::Stopped { .. } => Self::Cancelled,
            error => Self::Network {
                detail: error.to_string(),
            },
        }
    }
}
