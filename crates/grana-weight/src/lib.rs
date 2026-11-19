use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

mod archive;
mod cache;
mod download;
mod error;
mod integrity;
mod model;

pub use error::Error;
pub use model::{License, ModelVersion, license};

pub fn weights(
    version: ModelVersion,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, Error> {
    let asset = model::asset(version).ok_or_else(|| Error::unavailable(version.name()))?;
    let path = cache::stored(asset)?;
    if integrity::checked(asset, &path, cancel)? {
        grana_log::info!(target: "grana::weight", model = version.name(), path = %path.display(), "the model weights are ready");
        return Ok(path);
    }
    grana_log::info!(target: "grana::weight", model = version.name(), "downloading the model weights");
    download::fetch(asset, &path, cancel, progress)?;
    grana_log::info!(target: "grana::weight", model = version.name(), path = %path.display(), "the model weights were downloaded");
    Ok(path)
}
