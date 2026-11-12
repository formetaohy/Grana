use std::path::Path;

use serde::{Deserialize, Serialize};

use grana_protocol::Protocol;
use grana_report::{ComparisonReport, Quality, comparison_of};

use super::discover::{DatasetEntry, WellKey};
use super::resume;
use super::{Plan, Row};

pub(super) const RUN: &str = "run.json";

const CORRECTION: &str = "correction.json";

const FORMAT: u32 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    format: u32,
    build: String,
    protocol: u64,
    jobs: usize,
    project: bool,
    channels: bool,
    datasets: Vec<RunDataset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDataset {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stitched: Option<String>,
    group: String,
    title: String,
    scene: usize,
    scenes: usize,
    scene_name: Option<String>,
    well: Option<WellKey>,
    plate: Option<String>,
    folder: String,
    status: String,
    objects: usize,
    detail: String,
    content: u64,
    bytes: u64,
    modified: Option<u64>,
    protocol: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correction: Option<u64>,
    quality: Vec<Quality>,
}

impl Run {
    pub fn build(&self) -> &str {
        &self.build
    }

    pub fn protocol(&self) -> u64 {
        self.protocol
    }

    pub fn jobs(&self) -> usize {
        self.jobs
    }

    pub fn project(&self) -> bool {
        self.project
    }

    pub fn channels(&self) -> bool {
        self.channels
    }

    pub fn datasets(&self) -> &[RunDataset] {
        &self.datasets
    }

    pub fn comparison(&self) -> ComparisonReport {
        comparison_of(
            self.datasets
                .iter()
                .flat_map(|dataset| dataset.quality.iter()),
        )
    }
}

impl RunDataset {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn objects(&self) -> usize {
        self.objects
    }

    pub fn correction(&self) -> Option<u64> {
        self.correction
    }

    pub fn entry(&self) -> Option<DatasetEntry> {
        if self.scenes == 0 || self.scene >= self.scenes || self.title.is_empty() {
            return None;
        }
        Some(
            DatasetEntry::restored(
                Path::new(&self.path).to_path_buf(),
                self.group.clone(),
                self.scene,
                self.scenes,
                self.scene_name.clone(),
                self.well,
                self.plate.clone(),
            )
            .at_stitched(
                self.stitched
                    .as_ref()
                    .map(|path| Path::new(path).to_path_buf()),
            ),
        )
    }
}

pub(super) fn corrections(plan: &Plan) -> Result<Vec<Option<Protocol>>, String> {
    plan.datasets
        .iter()
        .map(|dataset| correction_at(&plan.output.join(dataset.folder())))
        .collect()
}

fn correction_at(folder: &Path) -> Result<Option<Protocol>, String> {
    let path = folder.join(CORRECTION);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("failed to read {}: {error}", path.display()));
        }
    };
    Protocol::from_json(&text)
        .map(Some)
        .map_err(|error| format!("{} is not a valid correction: {error}", path.display()))
}

pub fn record(run: &Path, dataset: &Path, scene: usize, protocol: &Protocol) -> Result<(), String> {
    let mut record = read(run)?.ok_or_else(|| format!("{} holds no run record", run.display()))?;
    let key = dataset.display().to_string();
    let entry = record
        .datasets
        .iter_mut()
        .find(|entry| entry.path == key && entry.scene == scene)
        .ok_or_else(|| format!("the run record does not name the dataset {key} scene {scene}"))?;
    let folder = run.join(&entry.folder);
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    let text = protocol
        .to_json()
        .map_err(|error| format!("the correction does not serialize: {error}"))?;
    let path = folder.join(CORRECTION);
    std::fs::write(&path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    entry.correction = Some(protocol.digest());
    write_run(run, &record)
}

pub fn clear(run: &Path, dataset: &Path, scene: usize) -> Result<(), String> {
    let mut record = read(run)?.ok_or_else(|| format!("{} holds no run record", run.display()))?;
    let key = dataset.display().to_string();
    let entry = record
        .datasets
        .iter_mut()
        .find(|entry| entry.path == key && entry.scene == scene)
        .ok_or_else(|| format!("the run record does not name the dataset {key} scene {scene}"))?;
    let folder = run.join(&entry.folder);
    let path = folder.join(CORRECTION);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("failed to remove {}: {error}", path.display()));
        }
    }
    entry.correction = None;
    write_run(run, &record)
}

pub(super) fn write(
    plan: &Plan,
    evidence: &[resume::Evidence],
    rows: &[Option<Row>],
    corrections: &[Option<Protocol>],
) -> Result<(), String> {
    let run = Run {
        format: FORMAT,
        build: resume::build(),
        protocol: plan.protocol.digest(),
        jobs: plan.jobs,
        project: plan.project,
        channels: plan.channels,
        datasets: plan
            .datasets
            .iter()
            .enumerate()
            .map(|(index, dataset)| {
                let row = rows.get(index).and_then(Option::as_ref);
                let held = evidence.get(index).copied().unwrap_or(resume::Evidence {
                    content: 0,
                    bytes: 0,
                    modified: None,
                });
                RunDataset {
                    path: dataset.path().display().to_string(),
                    stitched: dataset.stitched().map(|path| path.display().to_string()),
                    group: dataset.group().to_string(),
                    title: dataset.title(),
                    scene: dataset.scene(),
                    scenes: dataset.scenes(),
                    scene_name: dataset.scene_name(),
                    well: dataset.well_key(),
                    plate: dataset.plate_label().map(str::to_string),
                    folder: dataset.folder().display().to_string(),
                    status: row.map_or("queued", |row| row.status).to_string(),
                    objects: row.map_or(0, |row| row.objects),
                    detail: row.map_or_else(String::new, |row| row.detail.clone()),
                    content: held.content,
                    bytes: held.bytes,
                    modified: held.modified,
                    protocol: corrections
                        .get(index)
                        .and_then(Option::as_ref)
                        .unwrap_or(&plan.protocol)
                        .digest(),
                    correction: corrections
                        .get(index)
                        .and_then(Option::as_ref)
                        .map(Protocol::digest),
                    quality: row.map_or_else(Vec::new, |row| row.quality.clone()),
                }
            })
            .collect(),
    };
    write_run(&plan.output, &run)
}

fn write_run(output: &Path, run: &Run) -> Result<(), String> {
    let text = serde_json::to_string_pretty(run)
        .map_err(|error| format!("failed to record the batch run: {error}"))?;
    let path = output.join(RUN);
    grana_file::write(&path, text.as_bytes())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub fn read(output: &Path) -> Result<Option<Run>, String> {
    let path = output.join(RUN);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("failed to read {}: {error}", path.display()));
        }
    };
    let run = serde_json::from_str::<Run>(&text)
        .map_err(|error| format!("{} is not a valid run record: {error}", path.display()))?;
    if run.format != FORMAT {
        return Err(format!(
            "{} was written by another version of the run record",
            path.display()
        ));
    }
    Ok(Some(run))
}
