use super::*;

const FIELD_LIMIT: usize = 12;

pub(super) struct Field {
    pub(super) series: usize,
    pub(super) name: String,
    pub(super) source: Option<ProjectSource>,
    pub(super) fingerprint: Fingerprint,
    pub(super) scene: Scene,
    pub(super) measure: Measure,
    pub(super) history: History,
    pub(super) views: Vec<NamedView>,
    pub(super) view: Option<ViewState>,
    pub(super) timeline: Timeline,
    pub(super) export: Export,
    pub(super) coloc_thresholds: Option<(ColocPair, ColocThresholds)>,
    pub(super) mask_stamps: Vec<u64>,
    pub(super) project: Option<PathBuf>,
    pub(super) baseline: Option<u64>,
    pub(super) unsaved: bool,
}

pub(super) enum Recovered {
    Field(Box<Field>),
    Changed { kept: bool },
    Absent,
}

#[derive(Default)]
pub(super) struct Fields {
    entries: Vec<Field>,
    stale: Vec<Field>,
}

impl Fields {
    pub(super) fn take(&mut self, series: usize, fingerprint: Fingerprint) -> Recovered {
        let Some(position) = self.entries.iter().position(|field| field.series == series) else {
            return Recovered::Absent;
        };
        let field = self.entries.remove(position);
        if field.fingerprint == fingerprint {
            return Recovered::Field(Box::new(field));
        }
        let kept = field.unsaved;
        if kept {
            self.stale.push(field);
        }
        Recovered::Changed { kept }
    }

    pub(super) fn keep(&mut self, field: Field) -> Result<(), Box<Field>> {
        self.entries.retain(|held| held.series != field.series);
        self.entries.push(field);
        while self.entries.len() > FIELD_LIMIT {
            let Some(position) = self.entries.iter().position(|held| !held.unsaved) else {
                let rejected = self
                    .entries
                    .pop()
                    .expect("the cache holds the entry it just accepted");
                return Err(Box::new(rejected));
            };
            self.entries.remove(position);
        }
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.stale.clear();
    }

    pub(super) fn held(&self) -> Vec<Held> {
        self.entries
            .iter()
            .chain(self.stale.iter())
            .filter(|field| field.unsaved)
            .map(|field| Held {
                name: field.name.clone(),
                series: field.series,
                project: field.project.clone(),
                savable: field.source.is_some(),
            })
            .collect()
    }

    pub(super) fn find(&self, series: usize) -> Option<&Field> {
        self.entries
            .iter()
            .chain(self.stale.iter())
            .find(|field| field.series == series)
    }

    pub(super) fn find_mut(&mut self, series: usize) -> Option<&mut Field> {
        let Self { entries, stale } = self;
        entries
            .iter_mut()
            .find(|field| field.series == series)
            .or_else(|| stale.iter_mut().find(|field| field.series == series))
    }

    pub(super) fn remove(&mut self, series: usize) -> bool {
        let before = self.entries.len() + self.stale.len();
        self.entries.retain(|field| field.series != series);
        self.stale.retain(|field| field.series != series);
        self.entries.len() + self.stale.len() != before
    }

    pub(super) fn prune(&mut self) {
        self.stale.retain(|field| field.unsaved);
    }
}

impl Session {
    pub(super) fn capture_field(&mut self) -> Result<(), Held> {
        if self.loading.is_some() {
            return Ok(());
        }
        let unsaved = self.dirty();
        let Some(scene) = self.scene.take() else {
            return Ok(());
        };
        let fingerprint = self
            .fingerprint
            .take()
            .expect("a captured field names the dataset fingerprint it was opened with");
        let name = scene.dataset.name().to_string();
        let source = self.source_of(&scene.dataset, self.series_index);
        let view = self.current_view.take().map(|mut view| {
            view.timeline = self.timeline.clone();
            view.export = self.export;
            view
        });
        let field = Field {
            series: self.series_index,
            name,
            source,
            fingerprint,
            scene,
            measure: std::mem::take(&mut self.measure),
            history: std::mem::take(&mut self.history),
            views: std::mem::take(&mut self.views),
            view,
            timeline: self.timeline.clone(),
            export: self.export,
            coloc_thresholds: self.coloc_thresholds.take(),
            mask_stamps: std::mem::take(&mut self.mask_stamps),
            project: self.project.take(),
            baseline: self.baseline.take(),
            unsaved,
        };
        match self.fields.keep(field) {
            Ok(()) => Ok(()),
            Err(field) => {
                self.recover_field(*field);
                let held = self
                    .fields
                    .held()
                    .into_iter()
                    .next()
                    .expect("a full cache holds analysis to release");
                Err(held)
            }
        }
    }

    pub(super) fn recover_field(&mut self, field: Field) {
        let Field {
            fingerprint,
            scene,
            measure,
            history,
            views,
            view,
            timeline,
            export,
            coloc_thresholds,
            mask_stamps,
            project,
            baseline,
            ..
        } = field;
        self.fingerprint = Some(fingerprint);
        self.scene = Some(scene);
        self.measure = measure;
        self.history = history;
        self.views = views;
        self.view = view.map(|mut view| {
            view.timeline = timeline.clone();
            view.export = export;
            view
        });
        self.timeline = timeline;
        self.export = export;
        self.coloc_thresholds = coloc_thresholds;
        self.mask_stamps = mask_stamps;
        self.project = project;
        self.baseline = baseline;
    }

    pub(super) fn switching_wells(&mut self) -> bool {
        if self.analysing() {
            self.message = Some(Message::error(
                "wait for the running analysis before switching wells",
            ));
            return true;
        }
        false
    }

    pub fn held(&self) -> Vec<Held> {
        self.fields.held()
    }

    pub fn save_field(&mut self, series: usize, path: PathBuf) -> bool {
        if self.access().viewer() {
            self.message = Some(Message::info(
                "this viewer shows a recorded analysis and cannot save it",
            ));
            return false;
        }
        if self.tasks.transferring() {
            self.message = Some(Message::error(
                "wait for the running transfer before saving another field",
            ));
            return false;
        }
        let Some(field) = self.fields.find(series) else {
            self.message = Some(Message::error(format!(
                "the well cache holds no analysis of series {series}"
            )));
            return false;
        };
        let Some(source) = field.source.clone() else {
            self.message = Some(Message::error(
                "this dataset has no file or remote source to record in a project",
            ));
            return false;
        };
        let project = Project::capture(
            &field.scene,
            &field.measure,
            source,
            field_view(field),
            field.views.clone(),
            field.fingerprint,
        );
        let hash = field_hash(field);
        self.tasks.document = Some(Document::save_field(project, path, hash, series));
        self.message = None;
        true
    }

    pub fn discard_field(&mut self, series: usize) -> bool {
        if self.fields.remove(series) {
            return true;
        }
        self.message = Some(Message::error(format!(
            "the well cache holds no analysis of series {series}"
        )));
        false
    }

    pub(super) fn admit(&mut self) -> Option<Transfer> {
        let held = self.fields.held();
        (!held.is_empty()).then_some(Transfer::Held(held))
    }
}

pub(super) fn field_hash(field: &Field) -> Option<u64> {
    let views = field
        .views
        .iter()
        .fold(0u64, |hash, view| hash.rotate_left(7) ^ view.document());
    Some(
        field.scene.document_hash()
            ^ field.measure.document_hash()
            ^ field.timeline.document()
            ^ views
            ^ field.export.document(),
    )
}

fn field_view(field: &Field) -> ViewState {
    match field.view.clone() {
        Some(view) => view,
        None => {
            let mut camera = grana_display::Camera::new(35.0);
            camera.fit(field.scene.dataset.world_size(), 1.6);
            ViewState {
                camera: camera.state(),
                timeline: field.timeline.clone(),
                export: field.export,
            }
        }
    }
}
