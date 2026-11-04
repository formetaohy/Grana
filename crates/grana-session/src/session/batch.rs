use super::*;

impl Session {
    pub(super) fn add_batch_datasets(&mut self) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("Choose a folder of datasets")
            .pick_folder()
        else {
            return;
        };
        match grana_pipeline::discover(&root) {
            Ok(found) => {
                let mut added = 0;
                for dataset in found {
                    if !self.batch_datasets.contains(&dataset) {
                        self.batch_datasets.push(dataset);
                        added += 1;
                    }
                }
                if added == 0 {
                    self.batch_message = Some("the folder holds no new dataset".to_string());
                } else {
                    self.batch_message = None;
                }
                self.reset_batch_rows();
            }
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    pub(super) fn choose_batch_output(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a folder for the batch statistics")
            .pick_folder()
        else {
            return;
        };
        self.batch_output = Some(path);
        self.batch_message = None;
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
        self.batch_running = true;
        self.batch_message = None;
        self.batch = Some(Batch::start(BatchPlan {
            protocol,
            datasets: self.batch_datasets.clone(),
            output,
            jobs: self.batch_jobs,
            project: self.batch_project,
            snapshot: self.snapshotter.as_ref().map(|factory| factory()),
        }));
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
        loop {
            let event = match batch.poll() {
                BatchPoll::Event(event) => event,
                BatchPoll::Empty => break,
                BatchPoll::Closed => {
                    if !finished {
                        self.batch_running = false;
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
                BatchEvent::Failed(detail) => {
                    fatal = true;
                    self.batch_message = Some(detail);
                }
                BatchEvent::Done { cancelled } => {
                    finished = true;
                    self.batch_running = false;
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
