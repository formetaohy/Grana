use super::*;

impl Session {
    pub fn open_series(&mut self, name: &str, series: grana_volume::VolumeSeries) {
        let (source, pyramid) = Loader::memory(series);
        self.series = vec![Series::of(&pyramid)];
        self.series_index = 0;
        self.source = Some(source);
        self.scene = Some(Scene::new(Dataset::synthetic(name, pyramid)));
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
        self.set_time(0);
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
        if self.reject_transfer() {
            return;
        }
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file(path));
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.reset();
    }

    pub fn select_series(&mut self, index: usize) -> bool {
        if index == self.series_index || index >= self.series.len() {
            return false;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return false;
        };
        let Some(path) = source.path().map(Path::to_path_buf) else {
            return false;
        };
        if self.reject_transfer() {
            return false;
        }
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file_series(path, index));
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
        self.document = Some(Document::open(path, Arc::clone(&self.store)));
        self.message = None;
    }

    pub fn save_project(&mut self, path: PathBuf, view: ViewState) {
        if self.reject_transfer() {
            return;
        }
        let (Some(scene), Some(source)) = (self.scene.as_ref(), self.source_ref()) else {
            return;
        };
        let project = Project::capture(scene, &self.measure, source, view);
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
                let (source, pyramid) = Loader::memory(sample::series());
                self.source = Some(source);
                self.loading = None;
                self.pending = Some(Pending {
                    path: path.to_path_buf(),
                    project,
                });
                self.project = None;
                self.saved = None;
                self.reset();
                self.open_dataset(Dataset::synthetic(name, pyramid));
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
                    Source::File { path, series } => {
                        Some(ProjectSource::File { name, path, series })
                    }
                    Source::Memory(_) => None,
                }
            }
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
        self.history.clear();
        self.label_images.clear();
    }

    pub(super) fn open_dataset(&mut self, dataset: Dataset) {
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
        self.timeline = Timeline::new(30, self.frame_duration());
        self.history.clear();
        if let Some(pending) = self.pending.take() {
            if pending.project.matches(&scene.dataset) {
                scene.restore(pending.project.scene);
                self.measure.restore(pending.project.measure, &scene);
                self.export = pending.project.view.export;
                self.view = Some(pending.project.view);
                self.project = Some(pending.path);
                self.saved = Some(
                    scene.document_hash() ^ self.measure.document_hash() ^ self.timeline.document(),
                );
            } else {
                self.message = Some(Message::error(
                    "the project was saved from a dataset with a different shape, channels or duration",
                ));
            }
        }
        self.scene = Some(scene);
        self.generation += 1;
        self.refresh_labels();
        if let Some(source) = &self.source {
            source.set_time(0);
            if let Some(scene) = self.scene.as_ref() {
                source.set_drift(scene.drift().clone());
            }
        }
    }

    fn refresh_labels(&mut self) {
        self.label_images.clear();
        let Some(reader) = self.source.as_ref().and_then(Loader::reader) else {
            return;
        };
        match reader.labels() {
            Ok(labels) => {
                self.label_images = labels.names().map(str::to_string).collect();
            }
            Err(error) => self.message = Some(Message::error(error.to_string())),
        }
    }

    pub(super) fn frame_duration(&self) -> f32 {
        self.scene
            .as_ref()
            .map(|scene| {
                (scene.dataset.frame_count() as f32 * scene.dataset.frame_interval())
                    .clamp(1.0, Timeline::MAX_DURATION)
            })
            .unwrap_or(1.0)
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
                LoaderEvent::Opened {
                    name,
                    pyramid,
                    format,
                    series,
                    selected,
                } => {
                    self.series = series;
                    self.series_index = selected;
                    let dataset = match format {
                        Format::Zarr => Dataset::zarr(name, pyramid),
                        Format::Tiff => Dataset::tiff(name, pyramid),
                        Format::Czi => Dataset::czi(name, pyramid),
                        Format::Nikon => Dataset::nikon(name, pyramid),
                        Format::Leica => Dataset::leica(name, pyramid),
                    };
                    self.open_dataset(dataset);
                }
                LoaderEvent::Brick { request, data } => {
                    self.in_flight.remove(&request);
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
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.in_flight.clear();
                    self.project = None;
                    self.saved = None;
                }
            }
        }
        repaint
    }
}
