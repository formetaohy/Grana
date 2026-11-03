use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the project `{path}`: {detail}")]
    Read { path: PathBuf, detail: String },
    #[error("failed to write the project `{path}`: {detail}")]
    Write { path: PathBuf, detail: String },
    #[error("the project `{path}` is invalid: {detail}")]
    Format { path: PathBuf, detail: String },
    #[error("the project transfer was cancelled")]
    Cancelled,
}
