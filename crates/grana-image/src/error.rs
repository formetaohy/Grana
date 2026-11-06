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
    #[error(transparent)]
    Readout(#[from] grana_readout::Error),
    #[error("failed to open the image `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the image `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the image cannot resolve the request: {detail}")]
    Resolved { detail: String },
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}
