use super::*;

use crate::task::Progress;

pub(super) enum Scan {
    Discovered(Vec<grana_pipeline::DatasetEntry>),
    Run { folder: PathBuf, read: Box<RunRead> },
}

pub(super) struct RunRead {
    protocol: Protocol,
    datasets: Vec<grana_pipeline::DatasetEntry>,
    rows: Vec<BatchDataset>,
    metrics: Vec<BatchMetric>,
    comparison: Vec<BatchComparison>,
    significance: Vec<BatchSignificance>,
    interactions: Vec<BatchInteraction>,
    jobs: usize,
    project: bool,
    channels: bool,
}

impl Session {
    pub(super) fn add_batch_datasets(&mut self) {
        self.ask(
            Inquiry::folder("Choose a folder of datasets"),
            |session, path| {
                let Some(root) = path else {
                    return;
                };
                session.scan_batch_folder(root);
            },
        );
    }

    fn scan_batch_folder(&mut self, root: PathBuf) {
        let tiles = self
            .protocol
            .as_ref()
            .is_some_and(|protocol| protocol.stitch().is_some());
        let task = Progress::spawn(grana_exec::Role::Read, move |_, _| {
            grana_pipeline::discover(&root, tiles)
                .map(Scan::Discovered)
                .map_err(|error| error.to_string())
        });
        self.tasks.batch = Some(task);
        self.batch_message = None;
    }

    fn install_batch_datasets(&mut self, found: Vec<grana_pipeline::DatasetEntry>) {
        let mut added = 0;
        for dataset in found {
            if !self.batch_datasets.contains(&dataset) {
                self.batch_datasets.push(dataset);
                added += 1;
            }
        }
        self.batch_message = if added == 0 {
            Some("the folder holds no new dataset".to_string())
        } else {
            None
        };
        self.reset_batch_rows();
    }

    pub(super) fn choose_batch_output(&mut self) {
        self.ask(
            Inquiry::folder("Choose a folder for the batch statistics"),
            |session, path| {
                let Some(path) = path else {
                    return;
                };
                session.batch_output = Some(path);
                session.batch_message = None;
            },
        );
    }

    pub(super) fn open_batch_run(&mut self) {
        if self.batch_running {
            return;
        }
        self.ask(Inquiry::folder("Open a batch run"), |session, path| {
            let Some(folder) = path else {
                return;
            };
            session.read_batch_run(folder);
        });
    }

    fn read_batch_run(&mut self, folder: PathBuf) {
        let task = Progress::spawn(grana_exec::Role::Read, move |_, _| read_run(folder));
        self.tasks.batch = Some(task);
    }

    fn install_batch_run(&mut self, folder: PathBuf, read: RunRead) {
        self.batch_output = Some(folder);
        self.batch_datasets = read.datasets;
        self.batch_rows = read.rows;
        self.batch_metrics = read.metrics;
        self.batch_comparison = read.comparison;
        self.batch_significance = read.significance;
        self.batch_interactions = read.interactions;
        self.batch_jobs = read.jobs;
        self.batch_project = read.project;
        self.batch_channels = read.channels;
        self.batch_review = None;
        self.batch_review_pending = None;
        self.protocol_text = Some(read.protocol.summary());
        self.protocol = Some(read.protocol);
        self.batch_message = None;
    }

    pub(super) fn poll_batch_scan(&mut self) -> bool {
        let Some(slot) = self.tasks.batch.as_mut() else {
            return false;
        };
        match slot.poll() {
            Poll::Event(Ok(Scan::Discovered(found))) => {
                self.tasks.batch = None;
                self.install_batch_datasets(found);
                true
            }
            Poll::Event(Ok(Scan::Run { folder, read })) => {
                self.tasks.batch = None;
                self.install_batch_run(folder, *read);
                true
            }
            Poll::Event(Err(detail)) => {
                self.tasks.batch = None;
                self.batch_message = Some(detail);
                true
            }
            Poll::Empty => false,
            Poll::Closed => {
                self.tasks.batch = None;
                self.batch_message = Some("the batch reader stopped".to_string());
                true
            }
        }
    }

    pub(super) fn review_batch_dataset(&mut self, index: usize) {
        if self.batch_running && !self.batch_paused {
            self.batch_message = Some("pause the batch before reviewing a dataset".to_string());
            return;
        }
        if self.loading.is_some() {
            return;
        }
        if self.dirty() {
            self.batch_message =
                Some("save the analysis before reviewing another dataset".to_string());
            return;
        }
        let Some(dataset) = self.batch_datasets.get(index) else {
            return;
        };
        let path = dataset.path().to_path_buf();
        let scene = dataset.scene();
        self.batch_review = None;
        self.batch_message = None;
        match self.begin_load(path.clone()) {
            Transfer::Started => {
                self.batch_review_pending = Some((index, path, scene));
            }
            Transfer::Held(held) => {
                self.batch_message = Some(format!(
                    "save the unsaved analysis of {} before reviewing another dataset",
                    held[0].name
                ));
            }
            Transfer::Refused => {}
        }
    }

    pub(super) fn record_batch_correction(&mut self) {
        if self.batch_running {
            self.batch_message =
                Some("finish the batch before recording its correction".to_string());
            return;
        }
        let Some((index, path, scene, fingerprint)) = self.batch_review.clone() else {
            self.batch_message =
                Some("review a dataset before recording its correction".to_string());
            return;
        };
        if self.fingerprint != Some(fingerprint) || self.scene.is_none() {
            self.batch_message = Some("the reviewed dataset left the session".to_string());
            return;
        }
        let Some(output) = self.batch_output.clone() else {
            self.batch_message = Some("choose the batch output folder first".to_string());
            return;
        };
        let protocol = match self.protocol_of() {
            Ok(protocol) => protocol,
            Err(detail) => {
                self.batch_message = Some(detail);
                return;
            }
        };
        match grana_pipeline::record_correction(&output, &path, scene, &protocol) {
            Ok(()) => {
                if let Some(row) = self.batch_rows.get_mut(index) {
                    row.state = "corrected".to_string();
                    row.detail = if row.detail.is_empty() {
                        "correction recorded".to_string()
                    } else {
                        format!("{} · correction recorded", row.detail)
                    };
                }
                self.batch_message = None;
            }
            Err(detail) => self.batch_message = Some(detail),
        }
    }

    pub(super) fn clear_batch_correction(&mut self) {
        if self.batch_running {
            self.batch_message =
                Some("finish the batch before clearing its correction".to_string());
            return;
        }
        let Some((index, path, scene, _)) = self.batch_review.clone() else {
            self.batch_message =
                Some("review a dataset before clearing its correction".to_string());
            return;
        };
        let Some(output) = self.batch_output.clone() else {
            self.batch_message = Some("choose the batch output folder first".to_string());
            return;
        };
        match grana_pipeline::clear_correction(&output, &path, scene) {
            Ok(()) => {
                if let Some(row) = self.batch_rows.get_mut(index) {
                    row.state = "queued".to_string();
                    row.detail.clear();
                    row.detail.push_str("the correction was cleared");
                }
                self.batch_message = None;
            }
            Err(detail) => self.batch_message = Some(detail),
        }
    }

    pub(super) fn review_ready(&self) -> bool {
        self.batch_review
            .as_ref()
            .is_some_and(|(_, _, _, fingerprint)| {
                self.scene.is_some() && self.fingerprint == Some(*fingerprint)
            })
    }

    pub(super) fn set_batch_jobs(&mut self, jobs: u32) {
        if self.batch_running {
            return;
        }
        self.batch_jobs = jobs.clamp(1, grana_pipeline::MAX_JOBS as u32) as usize;
    }

    pub(super) fn run_batch(&mut self) {
        if self.batch_running {
            return;
        }
        let Some(protocol) = self.protocol.clone() else {
            self.batch_message = Some("capture or load a protocol first".to_string());
            return;
        };
        if self.batch_datasets.is_empty() {
            self.batch_message = Some("add a folder of datasets first".to_string());
            return;
        }
        let Some(output) = self.batch_output.clone() else {
            self.batch_message = Some("choose an output folder first".to_string());
            return;
        };
        self.reset_batch_rows();
        self.batch_metrics.clear();
        self.batch_comparison.clear();
        self.batch_significance.clear();
        self.batch_interactions.clear();
        self.batch_running = true;
        self.batch_paused = false;
        self.batch_message = None;
        self.batch = Some(Batch::start(BatchPlan {
            protocol,
            datasets: self.batch_datasets.clone(),
            output,
            jobs: self.batch_jobs,
            project: self.batch_project,
            channels: self.batch_channels,
            capture: self.capturer.as_ref().map(|factory| factory()),
        }));
    }

    pub(super) fn set_batch_paused(&mut self, paused: bool) {
        let Some(batch) = &self.batch else {
            return;
        };
        let control = batch.control();
        if paused {
            control.pause();
        } else {
            control.resume();
        }
        self.batch_paused = control.paused();
        self.batch_message = None;
    }

    pub(super) fn reset_batch_rows(&mut self) {
        self.batch_rows = self
            .batch_datasets
            .iter()
            .map(|dataset| BatchDataset {
                name: dataset.label(),
                state: "queued".to_string(),
                detail: String::new(),
            })
            .collect();
    }

    pub(super) fn poll_batch(&mut self) -> bool {
        let Some(batch) = self.batch.take() else {
            return false;
        };
        let mut repaint = false;
        let mut finished = false;
        let mut fatal = false;
        let mut budget = UPLOAD_BATCH;
        while budget > 0 {
            budget -= 1;
            let event = match batch.poll() {
                BatchPoll::Event(event) => event,
                BatchPoll::Empty => break,
                BatchPoll::Closed => {
                    if !finished {
                        self.batch_running = false;
                        self.batch_paused = false;
                        self.batch_message = Some("the batch worker stopped".to_string());
                    }
                    return repaint;
                }
            };
            repaint = true;
            match event {
                BatchEvent::Started { index, name } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.name = name;
                        row.state = "running".to_string();
                    }
                }
                BatchEvent::Step { index, label } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.state = label;
                    }
                }
                BatchEvent::Progress { index, done, total } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.detail = format!("{done}/{total}");
                    }
                }
                BatchEvent::Dataset { index, outcome } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        match outcome {
                            Outcome::Complete {
                                objects, resumed, ..
                            } => {
                                row.state = if resumed { "resumed" } else { "done" }.to_string();
                                row.detail = format!("{objects} objects");
                            }
                            Outcome::Failed { detail } => {
                                row.state = "failed".to_string();
                                row.detail = detail;
                            }
                        }
                    }
                }
                BatchEvent::Comparison(report) => {
                    self.batch_metrics = report.metrics;
                    self.batch_comparison = report.groups;
                    self.batch_significance = report.tests;
                    self.batch_interactions = report.interactions;
                }
                BatchEvent::Failed(detail) => {
                    fatal = true;
                    self.batch_message = Some(detail);
                }
                BatchEvent::Done { cancelled } => {
                    finished = true;
                    self.batch_running = false;
                    self.batch_paused = false;
                    if !fatal {
                        self.batch_message =
                            cancelled.then(|| "the batch was cancelled".to_string());
                    }
                }
            }
        }
        if !finished {
            self.batch = Some(batch);
        }
        repaint
    }
}

fn run_state(status: &str) -> &str {
    match status {
        "ok" => "done",
        other => other,
    }
}

fn read_run(folder: PathBuf) -> Result<Scan, String> {
    let Some(run) = grana_pipeline::read_run(&folder)? else {
        return Err(format!("{} holds no run record", folder.display()));
    };
    let protocol = grana_protocol::Protocol::read(&folder.join(grana_protocol::Protocol::FILE))
        .map_err(|error| format!("the run carries no readable protocol: {error}"))?;
    let mut datasets = Vec::with_capacity(run.datasets().len());
    for dataset in run.datasets() {
        let Some(entry) = dataset.entry() else {
            return Err(format!(
                "the run record names the dataset {} it cannot reopen",
                dataset.title()
            ));
        };
        datasets.push(entry);
    }
    let rows = run
        .datasets()
        .iter()
        .map(|dataset| BatchDataset {
            name: dataset.title().to_string(),
            state: if dataset.correction().is_some() {
                "corrected".to_string()
            } else {
                run_state(dataset.status()).to_string()
            },
            detail: dataset.detail().to_string(),
        })
        .collect();
    let report = run.comparison();
    Ok(Scan::Run {
        folder,
        read: Box::new(RunRead {
            protocol,
            datasets,
            rows,
            metrics: report.metrics,
            comparison: report.groups,
            significance: report.tests,
            interactions: report.interactions,
            jobs: run.jobs().clamp(1, grana_pipeline::MAX_JOBS),
            project: run.project(),
            channels: run.channels(),
        }),
    })
}
