use std::path::PathBuf;

use crate::Error;
use crate::model::Model;

pub(crate) fn stored() -> Result<PathBuf, Error> {
    Ok(directory()?.join(Model::FILE))
}

fn directory() -> Result<PathBuf, Error> {
    let base =
        grana_dir::cache().ok_or_else(|| Error::storage("the system holds no cache directory"))?;
    Ok(base.join("models").join("micro-sam"))
}
