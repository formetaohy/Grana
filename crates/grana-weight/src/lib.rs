use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

mod cache;
mod download;
mod error;
mod integrity;
mod model;

pub use error::Error;
pub use model::Model;

pub fn weights(cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, Error> {
    let asset = model::asset();
    let path = cache::stored()?;
    if integrity::checked(&asset, &path, cancel)? {
        grana_log::info!(target: "grana::weight", model = Model::NAME, path = %path.display(), "the model weights are ready");
        return Ok(path);
    }
    grana_log::info!(target: "grana::weight", model = Model::NAME, "downloading the model weights");
    download::fetch(&asset, &path, cancel, progress)?;
    grana_log::info!(target: "grana::weight", model = Model::NAME, path = %path.display(), "the model weights were downloaded");
    Ok(path)
}
