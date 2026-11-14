use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to create the spill file `{path}`: {detail}")]
    Create { path: PathBuf, detail: String },
    #[error("failed to read or write the spill file `{path}`: {detail}")]
    Spill { path: PathBuf, detail: String },
    #[error("the spill file `{path}` does not hold the payload it claims")]
    Format { path: PathBuf },
    #[error("the page handle is unknown to the store that issued it")]
    Unknown,
}
