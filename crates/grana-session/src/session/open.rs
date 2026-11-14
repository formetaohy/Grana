use super::*;

use std::time::Instant;

impl Session {
    pub fn open_series(&mut self, name: &str, series: grana_volume::VolumeSeries) {
        self.read_only = false;
        let (source, opened) = Loader::memory(series);
        self.series = opened.series;
        self.series_index = opened.selected;
        self.source = Some(source);
        self.scene = Some(Scene::new(Dataset::synthetic(name, opened.pyramid)));
        self.fingerprint = Some(opened.fingerprint);
        self.timeline = Timeline::new(30, self.frame_duration());
        self.message = None;
        self.loading = None;
        self.replace();
        self.project = None;
        self.baseline = None;
        self.history.clear();
        self.analysis.clear();
        self.progress.clear();
        self.recipe = None;
        self.resume.clear();
        self.recut = None;
        self.discard_streamed();
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
        self.baseline = self.document_hash();
    }

    pub fn set_capture_factory(&mut self, factory: CaptureFactory) {
        self.capturer = Some(factory);
    }

    pub fn begin_load(&mut self, path: PathBuf) {
        self.begin_load_series(path, 0);
    }

    pub fn begin_load_series(&mut self, path: PathBuf, series: usize) {
        self.batch_review_pending = None;
        self.stitch = None;
        if self.reject_transfer() {
            return;
        }
        self.read_only = false;
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file_series(path, series));
        self.pending = None;
        self.project = None;
        self.baseline = None;
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.reset();
    }

    pub fn begin_remote(&mut self, url: String) {
        self.begin_remote_series(url, 0);
    }

    pub fn begin_remote_series(&mut self, url: String, series: usize) {
        if self.reject_transfer() {
            return;
        }
        if !grana_image::is_remote(&url) {
            self.message = Some(Message::error(format!(
                "`{url}` is not an http or https url"
            )));
            return;
        }
        self.read_only = false;
        self.loading = Some(url.clone());
        self.source = Some(Loader::remote_series(url, series));
        self.pending = None;
        self.project = None;
        self.baseline = None;
        self.plate = None;
        self.field = 0;
        self.fields.clear();
        self.clear_plate();
        self.reset();
    }

    pub fn open_recent(&mut self, index: usize) {
        let Some(entry) = self.recent.entry(index).cloned() else {
            return;
        };
        match entry.kind() {
            crate::recent::Kind::Image => {
                self.begin_load_series(PathBuf::from(entry.target()), entry.series());
            }
            crate::recent::Kind::Project => {
                self.begin_project(PathBuf::from(entry.target()));
            }
            crate::recent::Kind::Remote => {
                self.begin_remote_series(entry.target().to_string(), entry.series());
            }
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        if let Some(project) = grana_project::package_project(&path) {
            self.begin_project(project);
            return;
        }
        if grana_project::is_project(&path) {
            self.begin_project(path);
            return;
        }
        if grana_image::is_image(&path) {
            self.begin_load(path);
            return;
        }
        if path.is_dir() && self.begin_stitch(path.clone()) {
            return;
        }
        self.message = Some(Message::error(format!(
            "{} is neither a supported image nor a grana project",
            path.display()
        )));
    }

    pub fn begin_package(&mut self, path: PathBuf) {
        let Some(project) = grana_project::package_project(&path) else {
            self.message = Some(Message::error(format!(
                "{} holds no recorded analysis",
                path.display()
            )));
            return;
        };
        self.begin_project(project);
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
        self.read_only = false;
        self.capture_field();
        self.loading = Some(source.label());
        self.source = Some(next);
        self.pending = None;
        self.project = None;
        self.baseline = None;
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
        self.read_only = false;
        self.capture_field();
        self.loading = Some(source.label());
        self.source = Some(next);
        self.pending = None;
        self.project = None;
        self.baseline = None;
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
        self.tasks.document = Some(Document::open(path, Arc::clone(&self.store)));
        self.message = None;
    }

    pub fn save_project(&mut self, path: PathBuf, view: ViewState) {
        if self.access().viewer() {
            self.message = Some(Message::info(
                "this viewer shows a recorded analysis and cannot save it",
            ));
            return;
        }
        if self.reject_transfer() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(source) = self.source_ref() else {
            self.message = Some(Message::error(
                "this dataset has no file or remote source to record in a project",
            ));
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
        self.tasks.document = Some(Document::save(project, path, hash));
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
        format!("{}.{}", crate::slug(name), grana_project::EXTENSION)
    }

    pub fn transferring(&self) -> bool {
        self.loading.is_some() || self.tasks.transferring()
    }

    pub(super) fn reject_transfer(&mut self) -> bool {
        if self.loading.is_some() {
            return true;
        }
        if self.tasks.transferring() {
            self.message = Some(Message::error("a project transfer is already running"));
            return true;
        }
        false
    }

    pub(super) fn open_project(&mut self, path: &Path, project: Project) {
        self.message = None;
        match &project.source {
            ProjectSource::Synthetic { .. } => {
                self.message = Some(Message::error(
                    "this project refers to an in-memory dataset, which the file does not contain",
                ));
            }
            ProjectSource::File {
                path: data, series, ..
            } => {
                let data = beside_project(path, data);
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
                self.baseline = None;
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
                self.baseline = None;
                self.reset();
            }
        }
    }

    pub(super) fn finish_document(
        &mut self,
        path: PathBuf,
        hash: Option<u64>,
        outcome: document::Outcome,
    ) {
        match outcome {
            document::Outcome::Saved => {
                self.project = Some(path.clone());
                self.baseline = hash;
                self.clear_recovery();
                self.record_recent(RecentEntry::project(&path));
            }
            document::Outcome::Opened(project) => {
                self.read_only = grana_project::in_package(&path);
                self.open_project(&path, *project);
                self.record_recent(RecentEntry::project(&path));
            }
            document::Outcome::Failed(detail) => {
                self.restore = None;
                self.message = Some(Message::error(detail));
            }
            document::Outcome::Cancelled => {
                self.restore = None;
            }
        }
    }

    pub(super) fn source_ref(&self) -> Option<ProjectSource> {
        let scene = self.scene.as_ref()?;
        let name = scene.dataset.name().to_string();
        match scene.dataset.origin() {
            Origin::Synthetic => None,
            Origin::Zarr
            | Origin::Tiff
            | Origin::Czi
            | Origin::Nikon
            | Origin::Leica
            | Origin::Imaris => match self.source.as_ref()?.source() {
                Source::File { path, series, .. } => {
                    Some(ProjectSource::File { name, path, series })
                }
                Source::Memory(_) | Source::Remote { .. } => None,
            },
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
        self.tasks.clear();
        self.labels_queue.clear();
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.deferred = false;
        self.recut = None;
        self.message = None;
        self.discard_streamed();
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
        self.discard_streamed();
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
        self.replace();
        self.mask_stamp = None;
        self.transfer.reset();
        if let Some(source) = &self.source {
            if let Some(scene) = self.scene.as_ref() {
                source.set_time(scene.time());
                source.set_drifts(scene.drifts().clone());
            } else {
                source.set_time(0);
            }
        }
        self.install_channels();
        if let Some(scene) = self.scene.as_ref() {
            self.mask_stamp = Some((scene.time(), scene.mask_stamp(scene.time())));
        }
        self.accept_open();
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
            let current = self.source.as_ref().map(Loader::source);
            match event {
                LoaderEvent::Opened(opened) => {
                    let opened = *opened;
                    let name = opened.name.clone();
                    let selected = opened.selected;
                    let restored = self.pending.is_some();
                    let imported = !restored && !opened.imports.is_empty();
                    let file = current
                        .as_ref()
                        .map(Source::label)
                        .unwrap_or_else(|| name.clone());
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
                        (false, Some(Format::Imaris)) => {
                            Dataset::imaris(opened.name, opened.pyramid)
                        }
                        (false, None) => Dataset::synthetic(opened.name, opened.pyramid),
                    };
                    self.open_dataset(dataset, opened.fingerprint);
                    if !restored {
                        self.import_scene(opened.imports, &file);
                    }
                    self.remember_open(current.as_ref(), &name, selected);
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
                    if imported {
                        self.baseline = self.document_hash();
                    }
                    if self.batch_review.is_some() {
                        self.baseline = None;
                        self.deferred = true;
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
                    self.restore = None;
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.channels = None;
                    self.in_flight.clear();
                    self.project = None;
                    self.baseline = None;
                    self.transfer.reset();
                }
            }
        }
        repaint
    }

    fn remember_open(&mut self, source: Option<&Source>, name: &str, series: usize) {
        let entry = match source {
            Some(Source::File { path, .. }) => RecentEntry::image(path, name, series),
            Some(Source::Remote { url, .. }) => RecentEntry::remote(url, name, series),
            Some(Source::Memory(_)) | None => return,
        };
        self.record_recent(entry);
    }
}

fn frame_duration(scene: &Scene) -> f32 {
    (scene.dataset.frame_count() as f32 * scene.dataset.frame_interval())
        .clamp(1.0, Timeline::MAX_DURATION)
}

fn beside_project(project: &Path, data: &Path) -> PathBuf {
    if data.is_absolute() && data.exists() {
        return data.to_path_buf();
    }
    let Some(folder) = project.parent() else {
        return data.to_path_buf();
    };
    let relative = folder.join(data);
    if relative.exists() {
        return relative;
    }
    let Some(name) = data.file_name() else {
        return data.to_path_buf();
    };
    let beside = folder.join(name);
    if beside.exists() {
        beside
    } else {
        data.to_path_buf()
    }
}
