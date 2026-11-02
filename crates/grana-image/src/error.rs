use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Zarr(#[from] grana_zarr::Error),
    #[error(transparent)]
    Tiff(#[from] grana_tiff::Error),
    #[error(transparent)]
    Czi(#[from] grana_czi::Error),
    #[error("failed to open the image `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the in-memory image cannot serve the request: {detail}")]
    Memory { detail: String },
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}
