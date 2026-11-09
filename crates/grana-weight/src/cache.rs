use std::path::PathBuf;

use crate::Error;
use crate::source::ModelVersion;

pub(crate) fn stored(version: ModelVersion) -> Result<PathBuf, Error> {
    Ok(directory()?.join(version.file()))
}

fn directory() -> Result<PathBuf, Error> {
    let base =
        grana_dir::data().ok_or_else(|| Error::storage("the system holds no data directory"))?;
    Ok(base.join("models"))
}
