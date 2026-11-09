use std::path::{Path, PathBuf};

use grana_scene::slug;

use super::DiscoverError;

const DEFAULT_JOBS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WellKey {
    row: usize,
    column: usize,
    field: usize,
}

impl WellKey {
    pub fn row(self) -> usize {
        self.row
    }

    pub fn column(self) -> usize {
        self.column
    }

    pub fn field(self) -> usize {
        self.field
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetEntry {
    path: PathBuf,
    group: String,
    scene: usize,
    scenes: usize,
    scene_name: Option<String>,
    well: Option<WellKey>,
    plate_label: Option<String>,
}

impl DatasetEntry {
    pub fn new(path: PathBuf, group: String) -> Self {
        Self {
            path,
            group,
            scene: 0,
            scenes: 1,
            scene_name: None,
            well: None,
            plate_label: None,
        }
    }

    pub fn scene_of(
        path: PathBuf,
        group: String,
        scene: usize,
        scenes: usize,
        scene_name: Option<String>,
    ) -> Self {
        assert!(
            scene < scenes && scenes > 1,
            "a scene is one of the several scenes of its image"
        );
        Self {
            path,
            group,
            scene,
            scenes,
            scene_name,
            well: None,
            plate_label: None,
        }
    }

    pub fn well(path: PathBuf, group: String, key: WellKey, field: String) -> Self {
        Self {
            path,
            group,
            scene: 0,
            scenes: 1,
            scene_name: None,
            well: Some(key),
            plate_label: Some(field),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn group(&self) -> &str {
        &self.group
    }

    pub fn scene(&self) -> usize {
        self.scene
    }

    pub fn scene_label(&self) -> String {
        match self.scenes {
            1 => String::new(),
            _ => self
                .scene_name
                .clone()
                .unwrap_or_else(|| self.scene.to_string()),
        }
    }

    pub fn well_key(&self) -> Option<WellKey> {
        self.well
    }

    pub fn well_row(&self) -> Option<usize> {
        self.well.map(|well| well.row())
    }

    pub fn well_column(&self) -> Option<usize> {
        self.well.map(|well| well.column())
    }

    pub fn well_field(&self) -> Option<usize> {
        self.well.map(|well| well.field())
    }

    pub fn plate_label(&self) -> Option<&str> {
        self.plate_label.as_deref()
    }

    pub fn name(&self) -> String {
        dataset_name(&self.path)
    }

    pub fn label(&self) -> String {
        let title = self.title();
        if self.group.is_empty() {
            title
        } else {
            format!("{}/{}", self.group, title)
        }
    }

    pub fn title(&self) -> String {
        if let Some(label) = self.plate_label.as_deref() {
            return label.to_string();
        }
        match self.scenes {
            1 => self.name(),
            _ => match &self.scene_name {
                Some(name) => format!("{} {}", self.name(), name),
                None => format!("{} scene {}", self.name(), self.scene + 1),
            },
        }
    }

    pub(super) fn folder(&self) -> PathBuf {
        let mut folder = PathBuf::new();
        for group in self.group.split('/').filter(|group| !group.is_empty()) {
            folder.push(slug(group).to_ascii_lowercase());
        }
        folder.push(slug(&self.title()).to_ascii_lowercase());
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
    if let Some(plate) = grana_image::plate_at(root).map_err(|error| DiscoverError::Read {
        path: root.display().to_string(),
        detail: error.to_string(),
    })? {
        return Ok(plate_entries(root, &plate));
    }
    if grana_image::is_image(root) {
        return Ok(entries_of(root, ""));
    }
    let mut datasets = Vec::new();
    walk(root, root, &mut datasets)?;
    datasets.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.well.cmp(&right.well))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.scene.cmp(&right.scene))
    });
    Ok(datasets)
}

fn plate_entries(root: &Path, plate: &grana_image::Plate) -> Vec<DatasetEntry> {
    let mut entries = Vec::with_capacity(plate.len());
    for well in plate.wells() {
        for field in well.fields() {
            let segments = field
                .path()
                .trim_start_matches('/')
                .split('/')
                .filter(|segment| !segment.is_empty());
            let mut path = root.to_path_buf();
            for segment in segments {
                path.push(segment);
            }
            entries.push(DatasetEntry::well(
                path,
                plate.name().to_string(),
                WellKey {
                    row: well.row(),
                    column: well.column(),
                    field: field.index(),
                },
                field.name().to_string(),
            ));
        }
    }
    entries
}

fn entries_of(path: &Path, group: &str) -> Vec<DatasetEntry> {
    let scenes = grana_image::series_count(path).unwrap_or(1);
    if scenes < 2 {
        return vec![DatasetEntry::new(path.to_path_buf(), group.to_string())];
    }
    let names = grana_image::series_names(path).unwrap_or_default();
    (0..scenes)
        .map(|scene| {
            DatasetEntry::scene_of(
                path.to_path_buf(),
                group.to_string(),
                scene,
                scenes,
                names.get(scene).cloned().flatten(),
            )
        })
        .collect()
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
            datasets.extend(entries_of(&path, &group_of(root, &entry.path())));
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
