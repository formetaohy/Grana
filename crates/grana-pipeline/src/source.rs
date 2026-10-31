use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::Movie;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<Movie>),
    File(PathBuf),
}

impl Source {
    pub fn memory(movie: Movie) -> Self {
        Self::Memory(Arc::new(movie))
    }

    pub fn file(path: PathBuf) -> Self {
        Self::File(path)
    }

    pub fn movie(&self) -> Option<&Arc<Movie>> {
        match self {
            Self::Memory(movie) => Some(movie),
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
