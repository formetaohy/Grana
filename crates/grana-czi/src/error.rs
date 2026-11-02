use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to open the CZI image `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the CZI metadata of `{path}` is invalid: {detail}")]
    Metadata { path: PathBuf, detail: String },
    #[error("the CZI image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the CZI region of `{path}` is invalid: {detail}")]
    Region { path: PathBuf, detail: String },
    #[error("failed to read the CZI data of `{path}`: {detail}")]
    Read { path: PathBuf, detail: String },
}

impl Error {
    pub(crate) fn open(path: &Path, detail: impl Into<String>) -> Self {
        Self::Open {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn metadata(path: &Path, detail: impl Into<String>) -> Self {
        Self::Metadata {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn unsupported(path: &Path, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn region(path: &Path, detail: impl Into<String>) -> Self {
        Self::Region {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn read(path: &Path, detail: impl Into<String>) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}
