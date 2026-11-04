mod discover;
mod pipeline;
mod quality;
mod resume;
mod sheet;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, MutexGuard};

use grana_protocol::Protocol;
use thiserror::Error;

use crate::snapshot::Snapshotter;
use discover::dataset_workers;
use pipeline::{analyse, artifacts};
use quality::Row;
use resume::{build, completed, content};

pub use discover::{DatasetEntry, dataset_name, default_jobs, discover};

const RESULT: &str = "result.json";

const PARTIAL: &str = "partial";

const SNAPSHOT: &str = "snapshot.png";

pub const MAX_JOBS: usize = 16;

const ANALYSIS_REVISION: u32 = 5;

pub struct Plan {
    pub protocol: Protocol,
    pub datasets: Vec<DatasetEntry>,
    pub output: PathBuf,
    pub jobs: usize,
    pub project: bool,
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
}

impl Batch {
    pub fn start(plan: Plan) -> Self {
        let (events, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancel);
        std::thread::spawn(move || execute(plan, events, token));
        Self {
            events: receiver,
            cancel,
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

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct Task<'a> {
    protocol: &'a Protocol,
    workers: usize,
    digest: u64,
    index: usize,
    project: bool,
    snapshot: Option<&'a dyn Snapshotter>,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
}

struct Progress<'a> {
    index: usize,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
}

fn execute(plan: Plan, events: Sender<Event>, cancel: Arc<AtomicBool>) {
    if let Err(detail) = prepare(&plan) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    let digest = plan.protocol.digest();
    let next = AtomicUsize::new(0);
    let rows = Mutex::new((0..plan.datasets.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = plan.jobs.clamp(1, MAX_JOBS).min(plan.datasets.len().max(1));
    let workers = dataset_workers(jobs);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let events = events.clone();
            let cancel = Arc::clone(&cancel);
            let next = &next;
            let rows = &rows;
            let plan = &plan;
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= plan.datasets.len() {
                        break;
                    }
                    work(
                        plan,
                        &Task {
                            protocol: &plan.protocol,
                            workers,
                            digest,
                            index,
                            project: plan.project,
                            snapshot: plan.snapshot.as_deref(),
                            events: &events,
                            cancel: &cancel,
                        },
                        rows,
                    );
                }
            });
        }
    });
    let mut summary = vec![grana_measure::csv::line(&[
        "dataset", "scene", "status", "objects", "detail",
    ])];
    let rows = lock(&rows);
    for (index, row) in rows.iter().enumerate() {
        if let Some(row) = row {
            let dataset = &plan.datasets[index];
            summary.push(row_line(
                &dataset.name(),
                &dataset.scene_label(),
                row.status,
                row.objects,
                &row.detail,
            ));
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
    if let Err(error) = std::fs::write(
        plan.output.join("comparison.csv"),
        quality::comparison(&plan, &rows),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison: {error}"
        )));
    }
    if let Err(detail) = sheet::write(&plan, &rows) {
        let _ = events.send(Event::Failed(detail));
    }
    drop(rows);
    let _ = events.send(Event::Done {
        cancelled: cancel.load(Ordering::Relaxed),
    });
}

fn prepare(plan: &Plan) -> Result<(), String> {
    if plan.protocol.snapshot().is_some() {
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
    let name = dataset.name();
    let folder = plan.output.join(dataset.folder());
    let artifacts = artifacts(task.protocol, &name, task.project);
    if let Some(record) = completed(
        &folder,
        task.digest,
        &build(),
        content(dataset.path()),
        &artifacts,
    ) {
        let _ = task.events.send(Event::Dataset {
            index,
            outcome: Outcome::Complete {
                objects: record.objects,
                detail: record.detail.clone(),
                resumed: true,
            },
        });
        store(
            rows,
            index,
            Row {
                status: "resumed",
                objects: record.objects,
                detail: record.detail,
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
            let _ = task.events.send(Event::Dataset {
                index,
                outcome: Outcome::Complete {
                    objects: report.objects,
                    detail: report.detail.clone(),
                    resumed: false,
                },
            });
            store(
                rows,
                index,
                Row {
                    status: "ok",
                    objects: report.objects,
                    detail: report.detail,
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

fn row_line(dataset: &str, scene: &str, status: &str, objects: usize, detail: &str) -> String {
    grana_measure::csv::line(&[dataset, scene, status, &objects.to_string(), detail])
}

fn lock(rows: &Mutex<Vec<Option<Row>>>) -> MutexGuard<'_, Vec<Option<Row>>> {
    rows.lock().unwrap_or_else(|error| error.into_inner())
}

fn store(rows: &Mutex<Vec<Option<Row>>>, index: usize, row: Row) {
    lock(rows)[index] = Some(row);
}
