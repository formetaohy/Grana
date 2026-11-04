use std::path::{Path, PathBuf};

use grana_scene::slug;

use super::DiscoverError;

const DEFAULT_JOBS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetEntry {
    path: PathBuf,
    group: String,
}

impl DatasetEntry {
    pub fn new(path: PathBuf, group: String) -> Self {
        Self { path, group }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn group(&self) -> &str {
        &self.group
    }

    pub fn name(&self) -> String {
        dataset_name(&self.path)
    }

    pub fn label(&self) -> String {
        if self.group.is_empty() {
            self.name()
        } else {
            format!("{}/{}", self.group, self.name())
        }
    }

    pub(super) fn folder(&self) -> PathBuf {
        let mut folder = PathBuf::new();
        for group in self.group.split('/').filter(|group| !group.is_empty()) {
            folder.push(slug(group).to_ascii_lowercase());
        }
        folder.push(slug(&self.name()).to_ascii_lowercase());
        folder
    }

    pub(super) fn key(&self) -> String {
        self.folder()
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }
}

pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map_or(DEFAULT_JOBS, |cores| cores.get())
        .min(DEFAULT_JOBS)
}

pub(super) fn dataset_workers(jobs: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, |cores| cores.get())
        .div_ceil(jobs.max(1))
        .max(1)
}

pub fn discover(root: &Path) -> Result<Vec<DatasetEntry>, DiscoverError> {
    if grana_image::is_image(root) {
        return Ok(vec![DatasetEntry::new(root.to_path_buf(), String::new())]);
    }
    let mut datasets = Vec::new();
    walk(root, root, &mut datasets)?;
    datasets.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(datasets)
}

fn walk(root: &Path, folder: &Path, datasets: &mut Vec<DatasetEntry>) -> Result<(), DiscoverError> {
    let mut entries = std::fs::read_dir(folder)
        .map_err(|error| DiscoverError::Read {
            path: folder.display().to_string(),
            detail: error.to_string(),
        })?
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if grana_image::is_image(&path) {
            datasets.push(DatasetEntry::new(path, group_of(root, &entry.path())));
            continue;
        }
        let directory = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && !entry.file_name().to_string_lossy().starts_with('.');
        if directory {
            walk(root, &path, datasets)?;
        }
    }
    Ok(())
}

fn group_of(root: &Path, dataset: &Path) -> String {
    let Some(parent) = dataset.parent() else {
        return String::new();
    };
    let Ok(relative) = parent.strip_prefix(root) else {
        return String::new();
    };
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn dataset_name(path: &Path) -> String {
    grana_image::dataset_name(path)
}
