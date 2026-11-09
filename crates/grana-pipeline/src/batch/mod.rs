mod channel;
mod discover;
mod gate;
mod manifest;
mod pipeline;
mod quality;
mod resume;
mod sheet;
mod significance;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, MutexGuard, OnceLock};

use grana_protocol::Protocol;
use thiserror::Error;

use crate::snapshot::Snapshotter;
use discover::dataset_workers;
use gate::Gate;
use pipeline::{analyse, artifacts};
use quality::Row;
use resume::{build, completed, content};

pub use discover::{DatasetEntry, dataset_name, default_jobs, discover};
pub use manifest::{
    Run, RunDataset, clear as clear_correction, read as read_run, record as record_correction,
};
pub use quality::{Comparison, ComparisonReport, ComparisonSample, Metric, Quality};
pub use significance::{Significance, table as significance_table};

const RESULT: &str = "result.json";

const PARTIAL: &str = "partial";

const SNAPSHOT: &str = "snapshot.png";

pub const MAX_JOBS: usize = 16;

const ANALYSIS_REVISION: u32 = 10;

pub struct Plan {
    pub protocol: Protocol,
    pub datasets: Vec<DatasetEntry>,
    pub output: PathBuf,
    pub jobs: usize,
    pub project: bool,
    pub channels: bool,
    pub snapshot: Option<Arc<dyn Snapshotter>>,
}

pub enum Event {
    Started {
        index: usize,
        name: String,
    },
    Step {
        index: usize,
        label: String,
    },
    Progress {
        index: usize,
        done: usize,
        total: usize,
    },
    Dataset {
        index: usize,
        outcome: Outcome,
    },
    Comparison(ComparisonReport),
    Failed(String),
    Done {
        cancelled: bool,
    },
}

pub enum Outcome {
    Complete {
        objects: usize,
        detail: String,
        resumed: bool,
    },
    Failed {
        detail: String,
    },
}

pub enum BatchPoll {
    Event(Event),
    Empty,
    Closed,
}

#[derive(Debug, Error)]
pub enum DiscoverError {
    #[error("failed to read {path}: {detail}")]
    Read { path: String, detail: String },
}

pub struct Batch {
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    gate: Arc<Gate>,
}

#[derive(Clone)]
pub struct Control {
    cancel: Arc<AtomicBool>,
    gate: Arc<Gate>,
}

impl Control {
    pub fn pause(&self) {
        self.gate.pause();
    }

    pub fn resume(&self) {
        self.gate.resume();
    }

    pub fn paused(&self) -> bool {
        self.gate.paused()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.gate.resume();
    }
}

impl Batch {
    pub fn start(plan: Plan) -> Self {
        let (events, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = Gate::new();
        let token = Arc::clone(&cancel);
        let pause = Arc::clone(&gate);
        std::thread::spawn(move || execute(plan, events, token, pause));
        Self {
            events: receiver,
            cancel,
            gate,
        }
    }

    pub fn control(&self) -> Control {
        Control {
            cancel: Arc::clone(&self.cancel),
            gate: Arc::clone(&self.gate),
        }
    }

    pub fn poll(&self) -> BatchPoll {
        match self.events.try_recv() {
            Ok(event) => BatchPoll::Event(event),
            Err(TryRecvError::Empty) => BatchPoll::Empty,
            Err(TryRecvError::Disconnected) => BatchPoll::Closed,
        }
    }

    pub fn wait(&self) -> Option<Event> {
        self.events.recv().ok()
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.gate.resume();
    }
}

struct Task<'a> {
    protocol: &'a Protocol,
    workers: usize,
    digest: u64,
    content: u64,
    index: usize,
    project: bool,
    channels: bool,
    corrected: bool,
    snapshot: Option<&'a dyn Snapshotter>,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
    gate: &'a Gate,
}

struct Progress<'a> {
    index: usize,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
    gate: &'a Gate,
}

impl Progress<'_> {
    fn hold(&self) -> bool {
        self.gate.hold(self.cancel)
    }
}

fn execute(plan: Plan, events: Sender<Event>, cancel: Arc<AtomicBool>, gate: Arc<Gate>) {
    let corrections = match manifest::corrections(&plan) {
        Ok(corrections) => corrections,
        Err(detail) => {
            let _ = events.send(Event::Failed(detail));
            let _ = events.send(Event::Done { cancelled: false });
            return;
        }
    };
    if let Err(detail) = prepare(&plan, &corrections) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    if let Err(detail) = manifest::write(&plan, &[], &corrections) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    let contents = Mutex::new(HashMap::new());
    let next = AtomicUsize::new(0);
    let rows = Mutex::new((0..plan.datasets.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = plan.jobs.clamp(1, MAX_JOBS).min(plan.datasets.len().max(1));
    let workers = dataset_workers(jobs);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let events = events.clone();
            let cancel = Arc::clone(&cancel);
            let gate = Arc::clone(&gate);
            let next = &next;
            let rows = &rows;
            let plan = &plan;
            let contents = &contents;
            let corrections = &corrections;
            scope.spawn(move || {
                while gate.hold(&cancel) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= plan.datasets.len() {
                        break;
                    }
                    let content = content_of(contents, plan.datasets[index].path());
                    let protocol = corrections[index].as_ref().unwrap_or(&plan.protocol);
                    work(
                        plan,
                        &Task {
                            protocol,
                            workers,
                            digest: protocol.digest(),
                            content,
                            index,
                            project: plan.project,
                            channels: plan.channels,
                            corrected: corrections[index].is_some(),
                            snapshot: plan.snapshot.as_deref(),
                            events: &events,
                            cancel: &cancel,
                            gate: &gate,
                        },
                        rows,
                    );
                }
            });
        }
    });
    let mut summary = vec![grana_measure::csv::line(&[
        "dataset", "group", "well", "row", "column", "field", "status", "objects", "detail",
    ])];
    let rows = lock(&rows);
    for (index, row) in rows.iter().enumerate() {
        if let Some(row) = row {
            let dataset = &plan.datasets[index];
            summary.push(row_line(dataset, row));
        }
    }
    if let Err(error) = std::fs::write(plan.output.join("summary.csv"), summary.concat()) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch summary: {error}"
        )));
    }
    if let Err(error) = std::fs::write(
        plan.output.join("quality.csv"),
        quality::table(&plan, &rows),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch quality report: {error}"
        )));
    }
    let (report, csv) = quality::comparison(&rows);
    if let Err(error) = std::fs::write(plan.output.join("comparison.csv"), csv) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison: {error}"
        )));
    }
    if let Err(error) = std::fs::write(
        plan.output.join("comparison_significance.csv"),
        significance::table(&report.tests),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch significance: {error}"
        )));
    }
    if let Err(error) = std::fs::write(
        plan.output.join("samples.csv"),
        quality::samples(&report.groups),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison samples: {error}"
        )));
    }
    if let Err(detail) = manifest::write(&plan, &rows, &corrections) {
        let _ = events.send(Event::Failed(detail));
    }
    if let Err(detail) = sheet::write(&plan, &rows, &report) {
        let _ = events.send(Event::Failed(detail));
    }
    if let Err(detail) = sheet::significance(&plan, &report.tests) {
        let _ = events.send(Event::Failed(detail));
    }
    let _ = events.send(Event::Comparison(report));
    drop(rows);
    let _ = events.send(Event::Done {
        cancelled: cancel.load(Ordering::Relaxed),
    });
}

fn prepare(plan: &Plan, corrections: &[Option<Protocol>]) -> Result<(), String> {
    let snapshot = plan.protocol.snapshot().is_some()
        || corrections
            .iter()
            .flatten()
            .any(|protocol| protocol.snapshot().is_some());
    if snapshot {
        let snapshotter = plan.snapshot.as_ref().ok_or_else(|| {
            "the protocol requests quality snapshots but this run carries no renderer".to_string()
        })?;
        snapshotter.open()?;
    }
    std::fs::create_dir_all(&plan.output)
        .map_err(|error| format!("failed to create {}: {error}", plan.output.display()))?;
    plan.protocol
        .write(&plan.output.join("protocol.json"))
        .map_err(|error| error.to_string())?;
    let mut folders = Vec::new();
    for dataset in &plan.datasets {
        let folder = dataset.folder();
        if folders.contains(&folder) {
            return Err(format!(
                "two datasets share the output folder {}",
                folder.display()
            ));
        }
        folders.push(folder);
    }
    Ok(())
}

fn work(plan: &Plan, task: &Task<'_>, rows: &Mutex<Vec<Option<Row>>>) {
    let index = task.index;
    let dataset = &plan.datasets[index];
    let folder = plan.output.join(dataset.folder());
    let artifacts = artifacts(task.protocol, &dataset.title(), task.project, task.channels);
    if let Some(record) = completed(&folder, task.digest, &build(), task.content, &artifacts) {
        let detail = marked(record.detail.clone(), task.corrected);
        let _ = task.events.send(Event::Dataset {
            index,
            outcome: Outcome::Complete {
                objects: record.objects,
                detail: detail.clone(),
                resumed: true,
            },
        });
        store(
            rows,
            index,
            Row {
                status: "resumed",
                objects: record.objects,
                detail,
                quality: record.quality,
            },
        );
        return;
    }
    let _ = task.events.send(Event::Started {
        index,
        name: dataset.label(),
    });
    match analyse(task, dataset, &plan.output) {
        Ok(report) => {
            let detail = marked(report.detail, task.corrected);
            let _ = task.events.send(Event::Dataset {
                index,
                outcome: Outcome::Complete {
                    objects: report.objects,
                    detail: detail.clone(),
                    resumed: false,
                },
            });
            store(
                rows,
                index,
                Row {
                    status: "ok",
                    objects: report.objects,
                    detail,
                    quality: report.quality,
                },
            );
        }
        Err(detail) => {
            if task.cancel.load(Ordering::Relaxed) {
                return;
            }
            let _ = task.events.send(Event::Dataset {
                index,
                outcome: Outcome::Failed {
                    detail: detail.clone(),
                },
            });
            store(
                rows,
                index,
                Row {
                    status: "failed",
                    objects: 0,
                    detail,
                    quality: Vec::new(),
                },
            );
        }
    }
}

type Contents = Mutex<HashMap<PathBuf, Arc<OnceLock<u64>>>>;

fn content_of(contents: &Contents, path: &std::path::Path) -> u64 {
    let cell = {
        let mut known = contents.lock().unwrap_or_else(|error| error.into_inner());
        Arc::clone(known.entry(path.to_path_buf()).or_default())
    };
    *cell.get_or_init(|| content(path))
}

fn row_line(dataset: &DatasetEntry, row: &Row) -> String {
    let index = |value: Option<usize>| value.map_or_else(String::new, |value| value.to_string());
    grana_measure::csv::line(&[
        &dataset.title(),
        dataset.group(),
        dataset.plate_label().unwrap_or_default(),
        &index(dataset.well_row()),
        &index(dataset.well_column()),
        &index(dataset.well_field()),
        row.status,
        &row.objects.to_string(),
        &row.detail,
    ])
}

fn lock(rows: &Mutex<Vec<Option<Row>>>) -> MutexGuard<'_, Vec<Option<Row>>> {
    rows.lock().unwrap_or_else(|error| error.into_inner())
}

fn store(rows: &Mutex<Vec<Option<Row>>>, index: usize, row: Row) {
    lock(rows)[index] = Some(row);
}

fn marked(mut detail: String, corrected: bool) -> String {
    if corrected {
        detail.push_str(" (corrected)");
    }
    detail
}
