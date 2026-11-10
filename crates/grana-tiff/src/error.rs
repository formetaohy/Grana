use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to open the OME-TIFF image `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the OME-TIFF metadata of `{path}` is invalid: {detail}")]
    Metadata { path: PathBuf, detail: String },
    #[error("the OME-TIFF image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the OME-TIFF region of `{path}` is invalid: {detail}")]
    Region { path: PathBuf, detail: String },
    #[error("failed to read the OME-TIFF data of `{path}`: {detail}")]
    Read { path: PathBuf, detail: String },
    #[error("failed to write the OME-TIFF image `{path}`: {detail}")]
    Write { path: PathBuf, detail: String },
}

impl Error {
    pub(crate) fn unsupported(path: &std::path::Path, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn metadata(path: &std::path::Path, detail: impl Into<String>) -> Self {
        Self::Metadata {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn read(path: &std::path::Path, detail: impl Into<String>) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn open(path: &std::path::Path, detail: impl Into<String>) -> Self {
        Self::Open {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn write(path: &std::path::Path, detail: impl Into<String>) -> Self {
        Self::Write {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}
