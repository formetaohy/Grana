use std::path::Path;

use openreadout_core::FileInfo;
use openreadout_core::reader::{Dataset, Registry};

use crate::error::Error;
use crate::family::Family;

pub(crate) struct Document {
    pub(crate) family: Family,
    pub(crate) info: FileInfo,
    pub(crate) dataset: Box<dyn Dataset>,
}

pub(crate) fn registry() -> Registry {
    Registry::new()
        .with(Box::new(openreadout_nd2::Nd2Reader))
        .with(Box::new(openreadout_lif::LifReader))
        .with(Box::new(openreadout_hdf5::ImsReader))
}

pub(crate) fn open(path: &Path) -> Result<Document, Error> {
    if path.is_dir() {
        return Err(Error::unsupported(
            path,
            "the path is a directory and not a microscope file",
        ));
    }
    let (detection, dataset) = registry()
        .open(path)
        .map_err(|error| Error::open(path, error.to_string()))?;
    let family = Family::of(detection.format_id)
        .ok_or_else(|| Error::unsupported(path, "the file holds no supported microscope format"))?;
    let info = dataset
        .info()
        .map_err(|error| Error::open(path, error.to_string()))?;
    Ok(Document {
        family,
        info,
        dataset,
    })
}
