use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to open the microscope image `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the microscope image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the region of `{path}` is invalid: {detail}")]
    Region { path: PathBuf, detail: String },
    #[error("failed to read the microscope image `{path}`: {detail}")]
    Read { path: PathBuf, detail: String },
    #[error("failed to read the localization table `{path}`: {detail}")]
    Table { path: PathBuf, detail: String },
    #[error("failed to write the localization table `{path}`: {detail}")]
    Write { path: PathBuf, detail: String },
}

impl Error {
    pub(crate) fn open(path: &Path, detail: impl Into<String>) -> Self {
        Self::Open {
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

    pub(crate) fn table(path: &Path, detail: impl Into<String>) -> Self {
        Self::Table {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }

    pub(crate) fn write(path: &Path, detail: impl Into<String>) -> Self {
        Self::Write {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}
