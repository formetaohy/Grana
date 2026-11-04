use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the image `{path}`: {detail}")]
    Read { path: PathBuf, detail: String },
    #[error("the image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the image cannot be encoded: {detail}")]
    Encode { detail: String },
    #[error("failed to write the image `{path}`: {detail}")]
    Write { path: PathBuf, detail: String },
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}
