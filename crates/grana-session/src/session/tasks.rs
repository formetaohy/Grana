use std::path::PathBuf;

use grana_measure::Table;
use grana_pipeline::export::Summary;
use grana_scene::{MeasureKey, ObjectSetId, TableRequest};
use grana_zarr::Written;

use super::Session;
use super::dataset::DatasetExport;
use super::export::Csv;
use super::label::Labels;
use super::montage::Montage;
use super::provenance::Export as Provenance;
use crate::document::Document;
use crate::document::Outcome as DocumentOutcome;
use crate::task::{Poll, Progress};

pub(super) enum Completion {
    Measure {
        key: MeasureKey,
        table: Option<Table>,
    },
    Document {
        path: PathBuf,
        hash: Option<u64>,
        outcome: DocumentOutcome,
    },
    Labels {
        set: ObjectSetId,
        outcome: Result<Written, String>,
    },
    Csv(Result<PathBuf, String>),
    Provenance(Result<PathBuf, String>),
    Dataset(Result<Summary, String>),
    Montage(Result<PathBuf, String>),
}

pub(super) struct Measure {
    key: MeasureKey,
    task: Progress<Option<Table>>,
}

impl Measure {
    fn start(request: TableRequest) -> Self {
        let key = request.key();
        let task = Progress::spawn(grana_exec::Role::Compute, move |_, _| request.build());
        Self { key, task }
    }

    pub(super) fn poll(&mut self) -> Poll<Option<Table>> {
        self.task.poll()
    }

    pub(super) fn key(&self) -> MeasureKey {
        self.key
    }

    pub(super) fn stepped(&mut self) -> bool {
        self.task.stepped()
    }
}

#[derive(Default)]
pub(super) struct Tasks {
    pub measure: Option<Measure>,
    pub document: Option<Document>,
    pub labels: Option<Labels>,
    pub csv: Option<Csv>,
    pub provenance: Option<Provenance>,
    pub dataset: Option<DatasetExport>,
    pub montage: Option<Montage>,
    pending_csv: Option<PathBuf>,
}

impl Tasks {
    pub(super) fn poll(&mut self) -> (Vec<Completion>, bool) {
        let mut completions = Vec::new();
        let mut stepped = false;
        if let Some(slot) = self.measure.as_mut() {
            stepped |= slot.stepped();
            let key = slot.key();
            match slot.poll() {
                Poll::Event(table) => {
                    self.measure = None;
                    completions.push(Completion::Measure { key, table });
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.measure = None;
                    completions.push(Completion::Measure { key, table: None });
                }
            }
        }
        if let Some(slot) = self.document.as_mut() {
            stepped |= slot.stepped();
            let path = slot.path().to_path_buf();
            let hash = slot.hash();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.document = None;
                    completions.push(Completion::Document {
                        path,
                        hash,
                        outcome,
                    });
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.document = None;
                    completions.push(Completion::Document {
                        path,
                        hash,
                        outcome: DocumentOutcome::Failed(
                            "the project worker stopped before it finished".to_string(),
                        ),
                    });
                }
            }
        }
        if let Some(slot) = self.labels.as_mut() {
            stepped |= slot.stepped();
            let set = slot.set();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.labels = None;
                    completions.push(Completion::Labels { set, outcome });
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.labels = None;
                    completions.push(Completion::Labels {
                        set,
                        outcome: Err(
                            "the label export stopped before the image was written".to_string()
                        ),
                    });
                }
            }
        }
        if let Some(slot) = self.csv.as_mut() {
            stepped |= slot.stepped();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.csv = None;
                    completions.push(Completion::Csv(outcome));
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.csv = None;
                    completions.push(Completion::Csv(Err(
                        "the statistics export stopped before the file was written".to_string(),
                    )));
                }
            }
        }
        if let Some(slot) = self.provenance.as_mut() {
            stepped |= slot.stepped();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.provenance = None;
                    completions.push(Completion::Provenance(outcome));
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.provenance = None;
                    completions.push(Completion::Provenance(Err(
                        "the provenance worker stopped before the crate was written".to_string(),
                    )));
                }
            }
        }
        if let Some(slot) = self.dataset.as_mut() {
            stepped |= slot.stepped();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.dataset = None;
                    completions.push(Completion::Dataset(outcome));
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.dataset = None;
                    completions.push(Completion::Dataset(Err(
                        "the dataset export stopped before the image was written".to_string(),
                    )));
                }
            }
        }
        if let Some(slot) = self.montage.as_mut() {
            stepped |= slot.stepped();
            match slot.poll() {
                Poll::Event(outcome) => {
                    self.montage = None;
                    completions.push(Completion::Montage(outcome));
                }
                Poll::Empty => {}
                Poll::Closed => {
                    self.montage = None;
                    completions.push(Completion::Montage(Err(
                        "the plate montage stopped before the sheet was written".to_string(),
                    )));
                }
            }
        }
        (completions, stepped)
    }

    pub(super) fn writing(&self) -> bool {
        self.labels.is_some()
            || self.csv.is_some()
            || self.provenance.is_some()
            || self.dataset.is_some()
            || self.montage.is_some()
    }

    pub(super) fn transferring(&self) -> bool {
        self.document.is_some()
    }

    pub(super) fn measuring(&self) -> bool {
        self.measure.is_some()
    }

    pub(super) fn progress(&self) -> Option<(&str, usize, usize)> {
        if let Some(labels) = self.labels.as_ref() {
            let (done, total) = labels.progress();
            return Some((labels.label(), done, total));
        }
        if let Some(provenance) = self.provenance.as_ref() {
            let (done, total) = provenance.progress();
            return Some((provenance.label(), done, total));
        }
        if let Some(montage) = self.montage.as_ref() {
            let (done, total) = montage.progress();
            return Some((montage.label(), done, total));
        }
        if let Some(dataset) = self.dataset.as_ref() {
            let (done, total) = dataset.progress();
            return Some((dataset.label(), done, total));
        }
        self.csv.as_ref().map(|csv| {
            let (done, total) = csv.progress();
            (csv.label(), done, total)
        })
    }

    pub(super) fn document(&self) -> Option<(&str, usize, usize)> {
        let document = self.document.as_ref()?;
        let (done, total) = document.progress();
        Some((document.label(), done, total))
    }

    pub(super) fn start_measure(&mut self, request: TableRequest) {
        self.measure = Some(Measure::start(request));
    }

    pub(super) fn defer_csv(&mut self, path: PathBuf) {
        self.pending_csv = Some(path);
    }

    pub(super) fn pending_csv(&self) -> bool {
        self.pending_csv.is_some()
    }

    pub(super) fn take_csv(&mut self) -> Option<PathBuf> {
        self.pending_csv.take()
    }

    pub(super) fn clear(&mut self) {
        self.measure = None;
        self.document = None;
        self.labels = None;
        self.csv = None;
        self.provenance = None;
        self.dataset = None;
        self.montage = None;
        self.pending_csv = None;
    }
}

impl Session {
    pub(super) fn finish(&mut self, completion: Completion) {
        match completion {
            Completion::Measure { key, table } => self.finish_measure(key, table),
            Completion::Document {
                path,
                hash,
                outcome,
            } => {
                self.finish_document(path, hash, outcome);
            }
            Completion::Labels { set, outcome } => self.finish_labels(set, outcome),
            Completion::Csv(outcome) => self.finish_csv(outcome),
            Completion::Provenance(outcome) => self.finish_provenance(outcome),
            Completion::Dataset(outcome) => self.finish_dataset(outcome),
            Completion::Montage(outcome) => self.finish_montage(outcome),
        }
    }
}
