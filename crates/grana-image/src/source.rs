use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::VolumeSeries;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<VolumeSeries>),
    File { path: PathBuf, series: usize },
}

impl Source {
    pub fn memory(series: VolumeSeries) -> Self {
        Self::Memory(Arc::new(series))
    }

    pub fn file(path: PathBuf) -> Self {
        Self::File { path, series: 0 }
    }

    pub fn file_series(path: PathBuf, series: usize) -> Self {
        Self::File { path, series }
    }

    pub fn series(&self) -> usize {
        match self {
            Self::Memory(_) => 0,
            Self::File { series, .. } => *series,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory(_) => None,
            Self::File { path, .. } => Some(path),
        }
    }
}
