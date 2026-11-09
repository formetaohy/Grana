use super::*;

const FIELD_LIMIT: usize = 12;

pub(super) struct Field {
    pub(super) series: usize,
    pub(super) fingerprint: Fingerprint,
    pub(super) scene: Scene,
    pub(super) measure: Measure,
    pub(super) history: History,
    pub(super) views: Vec<NamedView>,
    pub(super) view: Option<ViewState>,
    pub(super) timeline: Timeline,
    pub(super) export: Export,
    pub(super) coloc_thresholds: Option<(ColocPair, ColocThresholds)>,
    pub(super) mask_stamp: Option<(u32, u64)>,
    pub(super) project: Option<PathBuf>,
    pub(super) saved: Option<u64>,
}

pub(super) enum Recovered {
    Field(Box<Field>),
    Changed,
    Absent,
}

#[derive(Default)]
pub(super) struct Fields {
    entries: Vec<Field>,
}

impl Fields {
    pub(super) fn take(&mut self, series: usize, fingerprint: Fingerprint) -> Recovered {
        let Some(position) = self.entries.iter().position(|field| field.series == series) else {
            return Recovered::Absent;
        };
        let field = self.entries.remove(position);
        if field.fingerprint != fingerprint {
            return Recovered::Changed;
        }
        Recovered::Field(Box::new(field))
    }

    pub(super) fn keep(&mut self, field: Field) {
        self.entries.retain(|held| held.series != field.series);
        self.entries.push(field);
        while self.entries.len() > FIELD_LIMIT {
            self.entries.remove(0);
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Session {
    pub(super) fn capture_field(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(scene) = self.scene.take() else {
            return;
        };
        let Some(fingerprint) = self.fingerprint.take() else {
            self.scene = Some(scene);
            return;
        };
        let view = self.current_view.take().map(|mut view| {
            view.timeline = self.timeline.clone();
            view.export = self.export;
            view
        });
        let field = Field {
            series: self.series_index,
            fingerprint,
            scene,
            measure: std::mem::take(&mut self.measure),
            history: std::mem::take(&mut self.history),
            views: std::mem::take(&mut self.views),
            view,
            timeline: self.timeline.clone(),
            export: self.export,
            coloc_thresholds: self.coloc_thresholds.take(),
            mask_stamp: self.mask_stamp.take(),
            project: self.project.take(),
            saved: self.saved.take(),
        };
        self.fields.keep(field);
    }

    pub(super) fn recover_field(&mut self, field: Field) {
        let Field {
            scene,
            measure,
            history,
            views,
            view,
            timeline,
            export,
            coloc_thresholds,
            mask_stamp,
            project,
            saved,
            ..
        } = field;
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
        self.mask_stamp = mask_stamp;
        self.project = project;
        self.saved = saved;
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
}
