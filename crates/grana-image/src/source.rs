use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::VolumeSeries;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<VolumeSeries>),
    File {
        path: PathBuf,
        series: usize,
        group: Option<String>,
    },
    Remote {
        url: String,
        series: usize,
        group: Option<String>,
    },
}

impl Source {
    pub fn memory(series: VolumeSeries) -> Self {
        Self::Memory(Arc::new(series))
    }

    pub fn file(path: PathBuf) -> Self {
        Self::File {
            path,
            series: 0,
            group: None,
        }
    }

    pub fn file_series(path: PathBuf, series: usize) -> Self {
        Self::File {
            path,
            series,
            group: None,
        }
    }

    pub fn file_group(path: PathBuf, series: usize, group: String) -> Self {
        Self::File {
            path,
            series,
            group: Some(group),
        }
    }

    pub fn remote(url: String) -> Self {
        Self::Remote {
            url,
            series: 0,
            group: None,
        }
    }

    pub fn remote_series(url: String, series: usize) -> Self {
        Self::Remote {
            url,
            series,
            group: None,
        }
    }

    pub fn remote_group(url: String, series: usize, group: String) -> Self {
        Self::Remote {
            url,
            series,
            group: Some(group),
        }
    }

    pub fn series(&self) -> usize {
        match self {
            Self::Memory(_) => 0,
            Self::File { series, .. } | Self::Remote { series, .. } => *series,
        }
    }

    pub fn group(&self) -> Option<&str> {
        match self {
            Self::Memory(_) => None,
            Self::File { group, .. } | Self::Remote { group, .. } => group.as_deref(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory(_) | Self::Remote { .. } => None,
            Self::File { path, .. } => Some(path),
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

    pub fn label(&self) -> String {
        match self {
            Self::Memory(_) => "synthetic".to_string(),
            Self::File { path, .. } => path
                .file_name()
                .and_then(|name| name.to_str())
                .map_or_else(|| path.display().to_string(), str::to_string),
            Self::Remote { url, .. } => url.clone(),
        }
    }
}
