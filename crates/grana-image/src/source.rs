use std::path::PathBuf;
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
}
