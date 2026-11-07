use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

mod archive;
mod cache;
mod download;
mod error;
mod source;

pub use error::Error;
pub use source::ModelVersion;

pub fn weights(
    version: ModelVersion,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, Error> {
    let path = cache::stored(version)?;
    if source::matches(version, &path) {
        archive::normalize(&path)?;
        return Ok(path);
    }
    download::fetch(version, &path, cancel, progress)?;
    Ok(path)
}
