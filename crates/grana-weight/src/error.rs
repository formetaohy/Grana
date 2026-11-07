use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the model weights cannot be stored: {detail}")]
    Storage { detail: String },
    #[error("the model weights cannot be downloaded: {detail}")]
    Download { detail: String },
    #[error("the model weights do not match the published checksum")]
    Checksum,
    #[error("the model weights hold an archive this platform cannot read: {detail}")]
    Archive { detail: String },
    #[error("the model download was cancelled")]
    Cancelled,
}

impl Error {
    pub(crate) fn archive(detail: impl Into<String>) -> Self {
        Self::Archive {
            detail: detail.into(),
        }
    }

    pub(crate) fn storage(detail: impl Into<String>) -> Self {
        Self::Storage {
            detail: detail.into(),
        }
    }

    pub(crate) fn download(detail: impl Into<String>) -> Self {
        Self::Download {
            detail: detail.into(),
        }
    }
}
