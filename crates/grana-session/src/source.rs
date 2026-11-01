use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::VolumeSeries;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<VolumeSeries>),
    File(PathBuf),
}

impl Source {
    pub fn memory(series: VolumeSeries) -> Self {
        Self::Memory(Arc::new(series))
    }

    pub fn file(path: PathBuf) -> Self {
        Self::File(path)
    }

    pub fn series(&self) -> Option<&Arc<VolumeSeries>> {
        match self {
            Self::Memory(series) => Some(series),
            Self::File(_) => None,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory(_) => None,
            Self::File(path) => Some(path),
        }
    }
}
