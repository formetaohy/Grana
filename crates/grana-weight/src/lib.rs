use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

mod cache;
mod download;
mod error;
mod source;

pub use error::Error;

pub fn weights(cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, Error> {
    let path = cache::stored()?;
    if source::matches(&path) {
        return Ok(path);
    }
    download::fetch(&path, cancel, progress)?;
    Ok(path)
}
