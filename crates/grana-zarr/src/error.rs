use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to open the OME-Zarr dataset `{path}`: {detail}")]
    Open { path: PathBuf, detail: String },
    #[error("the OME-Zarr metadata of `{path}` is invalid: {detail}")]
    Metadata { path: PathBuf, detail: String },
    #[error("the OME-Zarr dataset `{path}` is not supported: {detail}")]
    Unsupported { path: PathBuf, detail: String },
    #[error("the OME-Zarr region of `{array}` is invalid: {detail}")]
    Region { array: String, detail: String },
    #[error("failed to read the OME-Zarr array `{array}`: {detail}")]
    Read { array: String, detail: String },
    #[error("failed to write the OME-Zarr dataset `{path}`: {detail}")]
    Write { path: PathBuf, detail: String },
}
