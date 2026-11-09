use std::path::PathBuf;

use crate::Error;
use crate::model::Asset;

pub(crate) fn stored(asset: &Asset) -> Result<PathBuf, Error> {
    Ok(directory()?.join(asset.version.file()))
}

fn directory() -> Result<PathBuf, Error> {
    let base =
        grana_dir::data().ok_or_else(|| Error::storage("the system holds no data directory"))?;
    Ok(base.join("models"))
}
