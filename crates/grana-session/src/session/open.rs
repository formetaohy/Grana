use super::*;

use std::time::Instant;

impl Session {
    pub fn open_series(&mut self, name: &str, series: grana_volume::VolumeSeries) {
        let (source, opened) = Loader::memory(series);
        self.series = opened.series;
        self.series_index = opened.selected;
        self.source = Some(source);
        self.scene = Some(Scene::new(Dataset::synthetic(name, opened.pyramid)));
        self.fingerprint = Some(opened.fingerprint);
        self.timeline = Timeline::new(30, self.frame_duration());
        self.message = None;
        self.loading = None;
        self.generation += 1;
        self.project = None;
        self.saved = None;
        self.history.clear();
        self.analysis.clear();
        self.progress.clear();
        self.recipe = None;
        self.resume.clear();
        self.recut = None;
        self.in_flight.clear();
        self.bricks.clear();
        self.measure.reset();
        self.label_images.clear();
        self.mask_stamp = None;
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.set_time(0);
        self.install_channels();
        if let Some(scene) = self.scene.as_ref() {
            self.mask_stamp = Some((0, scene.mask_stamp(0)));
        }
    }

    pub fn open_sample(&mut self) {
        self.open_series("synthetic fluorescence", sample::series());
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let staged = sample::stage(scene);
        for set in [staged.spots, staged.surfaces, staged.filaments] {
            self.start_detect(set);
        }
    }

    pub fn set_snapshot_factory(&mut self, factory: SnapshotFactory) {
        self.snapshotter = Some(factory);
    }

    pub fn begin_load(&mut self, path: PathBuf) {
        self.batch_review_pending = None;
        if self.reject_transfer() {
            return;
        }
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file(path));
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.reset();
    }

    pub fn begin_remote(&mut self, url: String) {
        if self.reject_transfer() {
            return;
        }
        if !grana_image::is_remote(&url) {
            self.message = Some(Message::error(format!(
                "`{url}` is not an http or https url"
            )));
            return;
        }
        self.loading = Some(url.clone());
        self.source = Some(Loader::remote(url));
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.reset();
    }

    pub fn select_series(&mut self, index: usize) -> bool {
        if index == self.series_index || index >= self.series.len() {
            return false;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return false;
        };
        let next = match &source {
            Source::Memory(_) => return false,
            Source::File { path, .. } => Loader::file_series(path.clone(), index),
            Source::Remote { url, .. } => Loader::remote_series(url.clone(), index),
        };
        if self.reject_transfer() {
            return false;
        }
        if self.switching_wells() {
            return false;
        }
        self.capture_field();
        self.loading = Some(source.label());
        self.source = Some(next);
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.clear_plate();
        self.reset();
        true
    }

    pub fn select_field(&mut self, index: usize) -> bool {
        let Some(plate) = self.plate.as_ref() else {
            return false;
        };
        if index >= plate.len() {
            self.message = Some(Message::error(format!(
                "the plate holds {} fields of view and field {index} was requested",
                plate.len()
            )));
            return false;
        }
        if index == self.field {
            return false;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return false;
        };
        let next = match &source {
            Source::Memory(_) => return false,
            Source::File { path, .. } => Loader::file_series(path.clone(), index),
            Source::Remote { url, .. } => Loader::remote_series(url.clone(), index),
        };
        if self.reject_transfer() {
            return false;
        }
        if self.switching_wells() {
            return false;
        }
        self.capture_field();
        self.loading = Some(source.label());
        self.source = Some(next);
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.reset();
        true
    }

    pub fn begin_project(&mut self, path: PathBuf) {
        if self.reject_transfer() {
            return;
        }
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.document = Some(Document::open(path, Arc::clone(&self.store)));
        self.message = None;
    }

    pub fn save_project(&mut self, path: PathBuf, view: ViewState) {
        if self.reject_transfer() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(source) = self.source_ref() else {
            return;
        };
        let Some(fingerprint) = self.fingerprint else {
            self.message = Some(Message::error(
                "the loaded dataset carries no fingerprint for a project",
            ));
            return;
        };
        let project = Project::capture(
            scene,
            &self.measure,
            source,
            view,
            self.views.clone(),
            fingerprint,
        );
        let hash = self.document_hash();
        self.document = Some(Document::save(project, path, hash));
        self.message = None;
    }

    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    pub fn suggested_project(&self) -> String {
        let name = self
            .scene
            .as_ref()
            .map_or("project", |scene| scene.dataset.name());
        format!("{}.grana", crate::slug(name))
    }

    pub fn transferring(&self) -> bool {
        self.loading.is_some() || self.document.is_some()
    }

    pub(super) fn reject_transfer(&mut self) -> bool {
        if self.loading.is_some() {
            return true;
        }
        if self.document.is_some() {
            self.message = Some(Message::error("a project transfer is already running"));
            return true;
        }
        false
    }

    pub(super) fn open_project(&mut self, path: &Path, project: Project) {
        self.message = None;
        match &project.source {
            ProjectSource::Synthetic { name } => {
                let name = name.clone();
                let (source, opened) = Loader::memory(sample::series());
                self.source = Some(source);
                self.loading = None;
                self.pending = Some(Pending {
                    path: path.to_path_buf(),
                    project,
                });
                self.project = None;
                self.saved = None;
                self.reset();
                self.open_dataset(Dataset::synthetic(name, opened.pyramid), opened.fingerprint);
            }
            ProjectSource::File {
                path: data, series, ..
            } => {
                let data = data.clone();
                let series = *series;
                if !data.is_dir() && !data.is_file() {
                    self.message = Some(Message::error(format!(
                        "{} is not a readable image",
                        data.display()
                    )));
                    return;
                }
                self.source = Some(Loader::file_series(data.clone(), series));
                self.loading = Some(file_name(&data));
                self.pending = Some(Pending {
                    path: path.to_path_buf(),
                    project,
                });
                self.project = None;
                self.saved = None;
                self.reset();
            }
            ProjectSource::Remote { url, series, .. } => {
                let url = url.clone();
                let series = *series;
                self.source = Some(Loader::remote_series(url.clone(), series));
                self.loading = Some(url);
                self.pending = Some(Pending {
                    path: path.to_path_buf(),
                    project,
                });
                self.project = None;
                self.saved = None;
                self.reset();
            }
        }
    }

    pub(super) fn poll_document(&mut self) -> bool {
        let (events, closed, path, hash) = {
            let Some(document) = self.document.as_mut() else {
                return false;
            };
            let mut events = Vec::new();
            let mut closed = false;
            loop {
                match document.poll() {
                    Poll::Event(event) => events.push(event),
                    Poll::Empty => break,
                    Poll::Closed => {
                        closed = true;
                        break;
                    }
                }
            }
            (
                events,
                closed,
                document.path().to_path_buf(),
                document.hash(),
            )
        };
        if events.is_empty() && !closed {
            return false;
        }
        let mut settled = false;
        for event in events {
            settled = true;
            match event {
                document::Outcome::Saved => {
                    self.project = Some(path.clone());
                    self.saved = hash;
                }
                document::Outcome::Opened(project) => self.open_project(&path, *project),
                document::Outcome::Failed(detail) => {
                    self.message = Some(Message::error(detail));
                }
                document::Outcome::Cancelled => {}
            }
        }
        let finished = settled || closed;
        if finished {
            if closed && !settled {
                self.message = Some(Message::error(
                    "the project worker stopped before it finished",
                ));
            }
            self.document = None;
        }
        true
    }

    pub(super) fn source_ref(&self) -> Option<ProjectSource> {
        let scene = self.scene.as_ref()?;
        let name = scene.dataset.name().to_string();
        match scene.dataset.origin() {
            Origin::Synthetic => Some(ProjectSource::Synthetic { name }),
            Origin::Zarr | Origin::Tiff | Origin::Czi | Origin::Nikon | Origin::Leica => {
                match self.source.as_ref()?.source() {
                    Source::File { path, series, .. } => {
                        Some(ProjectSource::File { name, path, series })
                    }
                    Source::Memory(_) | Source::Remote { .. } => None,
                }
            }
            Origin::Remote => match self.source.as_ref()?.source() {
                Source::Remote { url, series, .. } => {
                    Some(ProjectSource::Remote { name, url, series })
                }
                Source::Memory(_) | Source::File { .. } => None,
            },
        }
    }

    pub fn document_hash(&self) -> Option<u64> {
        let scene = self.scene.as_ref()?;
        Some(scene.document_hash() ^ self.measure.document_hash() ^ self.timeline.document())
    }

    pub(super) fn reset(&mut self) {
        self.measure.reset();
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.recut = None;
        self.message = None;
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.timeline = Timeline::new(30, 1.0);
        self.view = None;
        self.views.clear();
        self.history.clear();
        self.label_images.clear();
        self.fingerprint = None;
        self.mask_stamp = None;
        self.channels = None;
        self.transfer.reset();
    }

    pub(super) fn open_dataset(&mut self, dataset: Dataset, fingerprint: Fingerprint) {
        let mut scene = Scene::new(dataset);
        self.loading = None;
        self.message = None;
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.measure.reset();
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.timeline = Timeline::new(30, frame_duration(&scene));
        self.history.clear();
        if let Some(pending) = self.pending.take() {
            match pending.project.matches(&scene.dataset, fingerprint) {
                Ok(()) => {
                    scene.restore(pending.project.scene);
                    self.measure.restore(pending.project.measure, &scene);
                    let view = pending.project.view;
                    self.views = pending.project.views;
                    self.timeline = view.timeline.clone();
                    self.export = view.export;
                    self.view = Some(view);
                    self.project = Some(pending.path);
                    self.saved = Some(
                        scene.document_hash()
                            ^ self.measure.document_hash()
                            ^ self.timeline.document(),
                    );
                }
                Err(mismatch) => {
                    self.message = Some(Message::error(match mismatch {
                        Mismatch::Dataset => "the project was saved from a different dataset",
                        Mismatch::Shape => {
                            "the project was saved from a dataset with a different shape, channels or duration"
                        }
                    }));
                }
            }
        }
        self.scene = Some(scene);
        match self.fields.take(self.series_index, fingerprint) {
            Recovered::Field(field) => self.recover_field(*field),
            Recovered::Changed => {
                self.message = Some(Message::info(
                    "the well changed since it was last open, so its analysis was reset",
                ));
            }
            Recovered::Absent => {}
        }
        self.fingerprint = Some(fingerprint);
        self.generation += 1;
        self.mask_stamp = None;
        self.transfer.reset();
        if let Some(source) = &self.source {
            if let Some(scene) = self.scene.as_ref() {
                source.set_time(scene.time());
                source.set_drift(scene.drift().clone());
            } else {
                source.set_time(0);
            }
        }
        self.install_channels();
        if let Some(scene) = self.scene.as_ref() {
            self.mask_stamp = Some((scene.time(), scene.mask_stamp(scene.time())));
        }
    }

    pub(super) fn frame_duration(&self) -> f32 {
        self.scene.as_ref().map_or(1.0, frame_duration)
    }

    pub(super) fn poll_source(&mut self) -> bool {
        let mut repaint = false;
        let mut processed = 0;
        while processed < UPLOAD_BATCH {
            let Some(event) = self.source.as_ref().and_then(Loader::poll) else {
                break;
            };
            processed += 1;
            repaint = true;
            match event {
                LoaderEvent::Opened(opened) => {
                    let opened = *opened;
                    self.series = opened.series;
                    self.series_index = opened.selected;
                    self.plate = opened.plate;
                    self.field = opened.field;
                    self.label_images = opened.labels;
                    let dataset = match (opened.remote, opened.format) {
                        (true, _) => Dataset::remote(opened.name, opened.pyramid),
                        (false, Some(Format::Zarr)) => Dataset::zarr(opened.name, opened.pyramid),
                        (false, Some(Format::Tiff)) => Dataset::tiff(opened.name, opened.pyramid),
                        (false, Some(Format::Czi)) => Dataset::czi(opened.name, opened.pyramid),
                        (false, Some(Format::Nikon)) => Dataset::nikon(opened.name, opened.pyramid),
                        (false, Some(Format::Leica)) => Dataset::leica(opened.name, opened.pyramid),
                        (false, None) => Dataset::synthetic(opened.name, opened.pyramid),
                    };
                    self.open_dataset(dataset, opened.fingerprint);
                    if let Some((index, path, scene)) = self.batch_review_pending.clone() {
                        if scene >= self.series.len() {
                            self.batch_review_pending = None;
                            self.message = Some(Message::error(
                                "the dataset does not hold the scene the run recorded",
                            ));
                        } else if self.series_index != scene {
                            if !self.select_series(scene) {
                                self.batch_review_pending = None;
                                self.message =
                                    Some(Message::error("the reviewed scene cannot be opened"));
                            }
                        } else {
                            self.batch_review_pending = None;
                            self.batch_review = Some((index, path, scene, opened.fingerprint));
                            self.apply_protocol();
                        }
                    }
                    self.start_previews();
                    if let Some(detail) = opened.labels_error {
                        self.message = Some(Message::error(detail));
                    }
                }
                LoaderEvent::Brick { request, data } => {
                    self.in_flight.remove(&request);
                    self.transfer.record(data.bytes(), Instant::now());
                    self.bricks.push((request, data));
                }
                LoaderEvent::ReadFailed { request, detail } => {
                    self.in_flight.remove(&request);
                    self.failed.insert(request);
                    self.message = Some(Message::error(format!(
                        "a volume region could not be read: {detail}"
                    )));
                }
                LoaderEvent::Failed(detail) => {
                    self.loading = None;
                    self.batch_review_pending = None;
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.channels = None;
                    self.in_flight.clear();
                    self.project = None;
                    self.saved = None;
                    self.transfer.reset();
                }
            }
        }
        repaint
    }
}

fn frame_duration(scene: &Scene) -> f32 {
    (scene.dataset.frame_count() as f32 * scene.dataset.frame_interval())
        .clamp(1.0, Timeline::MAX_DURATION)
}
