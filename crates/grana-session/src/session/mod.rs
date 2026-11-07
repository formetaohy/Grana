mod analysis;
mod batch;
mod cache;
mod edit;
mod execute;
mod export;
mod import;
mod interop;
mod label;
mod mask;
mod open;
mod replay;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_animation::{Keyframe, Timeline};
use grana_display::Shading;
use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::Format;
use grana_measure::{ColocPair, ColocThresholds, Group, Scope};
use grana_object::SurfaceCut;
use grana_object::TrackSet;
use grana_optics::PsfParams;
use grana_project::{Project, Source as ProjectSource, ViewState};
use grana_scene::{
    Criteria, Dataset, FilterRange, Item, Job, LassoPath, Measure, ObjectFrame, ObjectParams,
    ObjectSetId, Origin, Progress, RestoreSpec, Scene,
};
use grana_volume::{BrickData, BrickRequest, Region};

use crate::command::{Edit, Export};
use crate::document::{self, Document};
use crate::history::{History, Snapshot};
use crate::loader::{Event as LoaderEvent, Loader};
use crate::recipe::Recipe;
use crate::sample;
use crate::task::Poll;
use crate::view::{
    BatchComparison, BatchDataset, BatchView, HistoryView, IoView, Message, SeriesView, StoreView,
};
use crate::{Action, Analysis, Command, Series, Source};
use grana_pipeline::run::DetectRequest;
use grana_pipeline::{
    Batch, BatchEvent, BatchPoll, Event, Outcome, Plan as BatchPlan, SnapshotFactory,
};
use grana_protocol::{Protocol, Step, ViewSpec};

const MAX_IN_FLIGHT: usize = 8192;

const UPLOAD_BATCH: usize = 64;

struct Pending {
    path: PathBuf,
    project: Project,
}

#[derive(Default)]
pub struct Effects {
    pub open_dataset: bool,
    pub open_image: bool,
    pub open_project: bool,
    pub save_project: Option<bool>,
    pub snapshot: Option<Export>,
    pub movie: Option<Export>,
    pub sequence: Option<Export>,
    pub cancel_export: bool,
    pub keyframe: bool,
    pub repaint: bool,
}

pub struct Session {
    scene: Option<Scene>,
    store: Arc<GeometryStore>,
    measure: Measure,
    timeline: Timeline,
    source: Option<Loader>,
    analysis: Vec<Analysis>,
    progress: Vec<Progress>,
    in_flight: HashSet<BrickRequest>,
    failed: HashSet<BrickRequest>,
    bricks: Vec<(BrickRequest, BrickData)>,
    pending: Option<Pending>,
    loading: Option<String>,
    document: Option<Document>,
    message: Option<Message>,
    project: Option<PathBuf>,
    saved: Option<u64>,
    protocol: Option<Protocol>,
    protocol_text: Option<String>,
    recipe: Option<Recipe>,
    coloc_thresholds: Option<(ColocPair, ColocThresholds)>,
    current_view: Option<ViewState>,
    batch: Option<Batch>,
    batch_datasets: Vec<grana_pipeline::DatasetEntry>,
    batch_rows: Vec<BatchDataset>,
    batch_comparison: Vec<BatchComparison>,
    batch_output: Option<PathBuf>,
    batch_running: bool,
    batch_paused: bool,
    batch_jobs: usize,
    batch_project: bool,
    batch_message: Option<String>,
    generation: u64,
    reload: u64,
    view: Option<ViewState>,
    history: History,
    export: Export,
    labels: Option<label::Labels>,
    label_images: Vec<String>,
    resume: Vec<(ObjectSetId, ObjectSetId, i32)>,
    recut: Option<(ObjectSetId, SurfaceCut)>,
    snapshotter: Option<SnapshotFactory>,
    series: Vec<Series>,
    series_index: usize,
    mask_stamp: Option<(u32, u64)>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            scene: None,
            store: GeometryStore::temporary(DEFAULT_BUDGET)
                .expect("the geometry store needs a temporary file"),
            measure: Measure::new(),
            timeline: Timeline::new(30, 1.0),
            source: None,
            analysis: Vec::new(),
            progress: Vec::new(),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
            bricks: Vec::new(),
            pending: None,
            loading: None,
            document: None,
            message: None,
            project: None,
            saved: None,
            protocol: None,
            protocol_text: None,
            recipe: None,
            coloc_thresholds: None,
            current_view: None,
            batch: None,
            batch_datasets: Vec::new(),
            batch_rows: Vec::new(),
            batch_comparison: Vec::new(),
            batch_output: None,
            batch_running: false,
            batch_paused: false,
            batch_jobs: grana_pipeline::default_jobs(),
            batch_project: false,
            batch_message: None,
            generation: 0,
            reload: 0,
            view: None,
            history: History::default(),
            export: Export::NATIVE,
            labels: None,
            label_images: Vec::new(),
            resume: Vec::new(),
            recut: None,
            snapshotter: None,
            series: Vec::new(),
            series_index: 0,
            mask_stamp: None,
        }
    }
    pub fn insert_keyframe(&mut self, key: Keyframe) -> usize {
        let snapshot = if self.loading.is_none() {
            self.snapshot()
        } else {
            None
        };
        let index = self.timeline.insert(key);
        if let Some(snapshot) = snapshot {
            self.history.record(Edit::new(Action::Keyframe), snapshot);
        }
        index
    }
    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }
    pub fn measure(&self) -> &Measure {
        &self.measure
    }
    pub fn protocol(&self) -> Option<&Protocol> {
        self.protocol.as_ref()
    }
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }
    pub fn export(&self) -> Export {
        self.export
    }
    pub fn progress(&self) -> &[Progress] {
        &self.progress
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn reload(&self) -> u64 {
        self.reload
    }
    pub fn pending(&self) -> usize {
        self.in_flight.len()
    }
    pub fn loading(&self) -> Option<&str> {
        self.loading.as_deref()
    }
    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }
    pub fn project_name(&self) -> Option<String> {
        self.project.as_ref().map(|path| file_name(path))
    }
    pub fn dirty(&self) -> bool {
        self.project.is_some() && self.document_hash() != self.saved
    }
    pub fn history(&self) -> HistoryView {
        HistoryView {
            undo: self.history.undo_action(),
            redo: self.history.redo_action(),
        }
    }
    pub fn batch(&self) -> Option<BatchView<'_>> {
        Some(BatchView {
            protocol: self.protocol_text.as_deref(),
            datasets: &self.batch_rows,
            comparison: &self.batch_comparison,
            output: self
                .batch_output
                .as_ref()
                .map(|path| path.display().to_string()),
            jobs: self.batch_jobs,
            project: self.batch_project,
            running: self.batch_running,
            paused: self.batch_paused,
            message: self.batch_message.clone(),
        })
    }
    pub fn series(&self) -> SeriesView<'_> {
        SeriesView {
            entries: &self.series,
            selected: self.series_index,
        }
    }
    pub fn label_images(&self) -> &[String] {
        &self.label_images
    }
    pub fn io<'a>(
        &'a self,
        adapter: &'a str,
        export: Option<(u32, u32)>,
        export_message: Option<String>,
    ) -> IoView<'a> {
        let frames = self.scene.as_ref().map_or(0, |scene| scene.frame_count());
        let interval = self
            .scene
            .as_ref()
            .map_or(0.0, |scene| scene.dataset.frame_interval());
        let stats = self.store.stats();
        let document = self.document.as_ref().map(|document| {
            (
                document.label(),
                document.progress().0,
                document.progress().1,
            )
        });
        IoView {
            adapter,
            loading: self.loading.as_deref(),
            document,
            message: self.message.as_ref(),
            project: self.project_name(),
            dirty: self.dirty(),
            pending: self.in_flight.len(),
            frames,
            frame_interval: interval,
            export,
            export_message,
            label: self.label_progress(),
            store: StoreView {
                resident: stats.resident(),
                spilled: stats.spilled(),
            },
        }
    }
    pub fn restore_timeline(&mut self, timeline: Timeline) {
        self.timeline = timeline;
    }
    pub fn take_view(&mut self) -> Option<ViewState> {
        self.view.take()
    }
    pub fn observe_view(&mut self, view: ViewState) {
        self.current_view = Some(view);
    }
    pub fn take_bricks(&mut self) -> Vec<(BrickRequest, BrickData)> {
        std::mem::take(&mut self.bricks)
    }
    pub fn sync_measure(&mut self) {
        let busy = self.scene.is_some()
            && self.measure.set().is_some_and(|set| {
                self.analysis
                    .iter()
                    .any(|analysis| analysis.job().streams(set))
            });
        if let Some(scene) = self.scene.as_ref() {
            self.measure.set_roi(scene.annotations.roi_geometry());
            self.measure.sync(scene, busy);
        }
    }
    pub fn apply_measure(&mut self) {
        if self.measure.take_dirty()
            && let Some(scene) = self.scene.as_mut()
        {
            self.measure.apply(scene);
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut repaint = self.poll_source();
        repaint |= self.poll_document();
        repaint |= self.poll_analysis();
        repaint |= self.poll_batch();
        repaint |= self.poll_labels();
        repaint |= self.refresh_masks();
        repaint
    }
    pub fn request(&mut self, time: u32, bricks: &[BrickRequest]) {
        if self.loading.is_some() {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let mut batch = Vec::with_capacity(bricks.len().min(MAX_IN_FLIGHT));
        for request in bricks {
            if request.time != time || self.in_flight.contains(request) {
                continue;
            }
            if self.failed.contains(request) {
                continue;
            }
            if batch.len() >= MAX_IN_FLIGHT {
                break;
            }
            batch.push(*request);
        }
        if batch.is_empty() {
            return;
        }
        if source.request(&batch) {
            self.in_flight.extend(batch);
        } else {
            self.source = None;
            self.in_flight.clear();
            self.message = Some(Message::error("the dataset reader stopped"));
        }
    }
    pub fn set_time(&mut self, time: u32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if time == scene.time() {
            return;
        }
        scene.set_time(time);
        self.in_flight.retain(|request| request.time >= time);
        if let Some(source) = &self.source {
            source.set_time(time);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
