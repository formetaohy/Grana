mod channel;
mod discover;
mod gate;
mod manifest;
mod pipeline;
mod profile;
mod resume;
mod ro_crate;
mod sheet;
mod stitch;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use grana_exec::{Pipe, Poll, Sender};
use grana_protocol::Protocol;
use thiserror::Error;

use crate::capture::Capture;
use discover::dataset_workers;
use gate::Gate;
use pipeline::{analyse, artifacts};
use resume::{Evidence, build, completed, evidence};

pub use discover::{DatasetEntry, dataset_name, default_jobs, discover};
pub use grana_report::{
    Comparison, ComparisonReport, ComparisonSample, Interaction, Metric, Quality, Significance,
    interaction_table, significance_table,
};
pub use manifest::{
    Run, RunDataset, clear as clear_correction, read as read_run, record as record_correction,
};

pub(super) struct Row {
    pub(super) status: &'static str,
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
}

const RESULT: &str = "result.json";

const COMPARISON: &str = "comparison.svg";

const REPORT: &str = "report.pdf";

const PAGES: &str = "pages.json";

const TREND: &str = "comparison_trend.csv";

const PARTIAL: &str = "partial";

const CAPACITY: usize = 64;

pub const MAX_JOBS: usize = 16;

const ANALYSIS_REVISION: u32 = 18;

pub struct Plan {
    pub protocol: Protocol,
    pub datasets: Vec<DatasetEntry>,
    pub output: PathBuf,
    pub jobs: usize,
    pub project: bool,
    pub channels: bool,
    pub capture: Option<Arc<dyn Capture>>,
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
    events: Pipe<Event>,
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
        let (events, pipe) = Pipe::new(CAPACITY);
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = Gate::new();
        let token = Arc::clone(&cancel);
        let pause = Arc::clone(&gate);
        std::thread::Builder::new()
            .name("grana-batch".to_string())
            .spawn(move || execute(plan, events, token, pause))
            .expect("the batch coordinator starts");
        Self {
            events: pipe,
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
        match self.events.poll() {
            Poll::Event(event) => BatchPoll::Event(event),
            Poll::Empty => BatchPoll::Empty,
            Poll::Closed => BatchPoll::Closed,
        }
    }

    pub fn wait(&self) -> Option<Event> {
        self.events.receive()
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
    evidence: Evidence,
    index: usize,
    project: bool,
    channels: bool,
    corrected: bool,
    capture: Option<&'a dyn Capture>,
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
    let mut plan = plan;
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
    if let Err(detail) = restitch(&mut plan, &events, &cancel, &gate) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = events.send(Event::Done { cancelled: true });
        return;
    }
    let evidence = plan
        .datasets
        .iter()
        .map(|dataset| evidence(dataset.image_path()))
        .collect::<Vec<_>>();
    if let Err(detail) = manifest::write(&plan, &evidence, &[], &corrections) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
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
            let evidence = &evidence;
            let corrections = &corrections;
            scope.spawn(move || {
                while gate.hold(&cancel) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= plan.datasets.len() {
                        break;
                    }
                    let protocol = corrections[index].as_ref().unwrap_or(&plan.protocol);
                    work(
                        plan,
                        &Task {
                            protocol,
                            workers,
                            digest: protocol.digest(),
                            evidence: evidence[index],
                            index,
                            project: plan.project,
                            channels: plan.channels,
                            corrected: corrections[index].is_some(),
                            capture: plan.capture.as_deref(),
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
    if cancel.load(Ordering::Relaxed) {
        let mut rows = lock(&rows);
        for row in rows.iter_mut() {
            if row.is_none() {
                *row = Some(Row {
                    status: "cancelled",
                    objects: 0,
                    detail: "the run was cancelled".to_string(),
                    quality: Vec::new(),
                });
            }
        }
    }
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
    if let Err(error) = grana_file::write(
        &plan.output.join("summary.csv"),
        summary.concat().as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch summary: {error}"
        )));
    }
    if let Err(error) = grana_file::write(
        &plan.output.join("quality.csv"),
        grana_report::table(
            &plan
                .datasets
                .iter()
                .enumerate()
                .map(|(index, dataset)| grana_report::QualityDataset {
                    group: dataset.group().to_string(),
                    name: dataset.title(),
                    scene: dataset.scene_label(),
                    status: rows
                        .get(index)
                        .and_then(Option::as_ref)
                        .map_or("queued", |row| row.status),
                    quality: rows
                        .get(index)
                        .and_then(Option::as_ref)
                        .map_or(&[][..], |row| row.quality.as_slice()),
                })
                .collect::<Vec<_>>(),
        )
        .as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch quality report: {error}"
        )));
    }
    let (report, csv) = grana_report::comparison(rows.iter().flatten().flat_map(|row| {
        let settled = row.status == "ok" || row.status == "resumed";
        row.quality.iter().filter(move |_| settled)
    }));
    if let Err(error) = grana_file::write(&plan.output.join("comparison.csv"), csv.as_bytes()) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison: {error}"
        )));
    }
    if let Err(error) = grana_file::write(
        &plan.output.join("comparison_significance.csv"),
        grana_report::significance_table(&report.tests).as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch significance: {error}"
        )));
    }
    if let Err(error) = grana_file::write(
        &plan.output.join(TREND),
        grana_report::interaction_table(&report.interactions).as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch time interaction: {error}"
        )));
    }
    if let Err(error) = grana_file::write(
        &plan.output.join("comparison_frames.csv"),
        grana_report::comparison_frames(&report.metrics, &report.groups).as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison frames: {error}"
        )));
    }
    if let Err(error) = grana_file::write(
        &plan.output.join("samples.csv"),
        grana_report::samples(&report.metrics, &report.groups).as_bytes(),
    ) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch comparison samples: {error}"
        )));
    }
    let pages = grana_report::pages(
        &report,
        grana_chart::DistributionStyle::Box,
        pipeline::language(),
    );
    let recorded = plan
        .datasets
        .iter()
        .flat_map(|dataset| {
            grana_page::Recorded::read(&plan.output.join(dataset.folder()).join(PAGES))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    if (!pages.is_empty() || !recorded.is_empty())
        && let Err(detail) = grana_page::write_report(&pages, &recorded, &plan.output.join(REPORT))
    {
        let _ = events.send(Event::Failed(detail));
    }
    if !pages.is_empty() {
        let sheet = grana_chart::Page::Sheet(Box::new(grana_chart::Sheet::new(pages, 2)));
        if let Err(detail) = grana_page::write_page(&sheet, &plan.output.join(COMPARISON)) {
            let _ = events.send(Event::Failed(detail));
        }
    }
    if let Err(detail) = manifest::write(&plan, &evidence, &rows, &corrections) {
        let _ = events.send(Event::Failed(detail));
    }
    if let Err(detail) = sheet::write(&plan, &rows, &report) {
        let _ = events.send(Event::Failed(detail));
    }
    if let Err(detail) = sheet::significance(&plan, &report) {
        let _ = events.send(Event::Failed(detail));
    }
    if let Err(detail) = ro_crate::write(&plan, &rows) {
        let _ = events.send(Event::Failed(detail));
    }
    let _ = events.send(Event::Comparison(report));
    drop(rows);
    let _ = events.send(Event::Done {
        cancelled: cancel.load(Ordering::Relaxed),
    });
}

fn restitch(
    plan: &mut Plan,
    events: &Sender<Event>,
    cancel: &AtomicBool,
    gate: &Gate,
) -> Result<(), String> {
    let Some(spec) = plan.protocol.stitch().cloned() else {
        return Ok(());
    };
    let output = plan.output.clone();
    for (index, dataset) in plan.datasets.iter_mut().enumerate() {
        if !gate.hold(cancel) {
            return Ok(());
        }
        let progress = Progress {
            index,
            events,
            cancel,
            gate,
        };
        stitch::place(&output, dataset, &spec, &progress)?;
    }
    Ok(())
}

fn prepare(plan: &Plan, corrections: &[Option<Protocol>]) -> Result<(), String> {
    let captures = plan.protocol.snapshot().is_some()
        || corrections
            .iter()
            .flatten()
            .any(|protocol| protocol.snapshot().is_some());
    if captures {
        let capture = plan.capture.as_ref().ok_or_else(|| {
            "the protocol requests captures but this run carries no renderer".to_string()
        })?;
        capture.open()?;
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
    if let Some(record) = completed(
        &folder,
        task.digest,
        &build(),
        task.evidence.content,
        &artifacts,
    ) {
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
                store(
                    rows,
                    index,
                    Row {
                        status: "cancelled",
                        objects: 0,
                        detail: "the run was cancelled".to_string(),
                        quality: Vec::new(),
                    },
                );
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
