use std::path::PathBuf;

use crate::Error;
use crate::source::ModelVersion;

pub(crate) fn stored(version: ModelVersion) -> Result<PathBuf, Error> {
    Ok(directory()?.join(version.file()))
}

fn directory() -> Result<PathBuf, Error> {
    let base = base().ok_or_else(|| Error::storage("the system holds no data directory"))?;
    Ok(base.join("grana").join("models"))
}

#[cfg(target_os = "windows")]
fn base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn base() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn base() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}
