mod analysis;
mod batch;
mod dataset;
mod derived;
mod edit;
mod execute;
mod export;
mod field;
mod import;
mod intensity;
mod interop;
mod label;
mod mask;
mod montage;
mod open;
mod plate;
mod preview;
mod provenance;
mod recover;
mod replay;
mod scene_import;
mod seed;
mod tasks;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_animation::{Keyframe, Timeline};
use grana_display::Shading;
use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::Format;
use grana_image::Plate;
use grana_measure::{ColocPair, ColocThresholds, Group, Scope};
use grana_object::SurfaceCut;
use grana_object::TrackSet;
use grana_optics::Psf;
use grana_project::{Mismatch, NamedView, Project, Source as ProjectSource, ViewState};
use grana_scene::{
    Criteria, Dataset, FilterRange, Item, Job, LassoPath, Measure, ObjectFrame, ObjectParams,
    ObjectSetId, Origin, Progress, Revision, Revisions, Scene,
};
use grana_volume::{BrickData, BrickRequest, Fingerprint, Region};

use crate::command::{Edit, Export};
use crate::document::{self, Document};
use crate::history::{History, Snapshot};
use crate::loader::{Event as LoaderEvent, Loader};
use crate::recent::{Entry as RecentEntry, Recent};
use crate::recipe::Recipe;
use crate::recovery::Recovery;
use crate::task::Poll;
use crate::transfer::Transfer;
use crate::view::{
    BatchComparison, BatchDataset, BatchInteraction, BatchMetric, BatchSignificance, BatchView,
    HistoryView, ImportNote, IoView, Message, PlateView, SeriesView, StoreView,
};
use crate::{Action, Analysis, Command, Series, Source};
use field::{Fields, Recovered};
use grana_pipeline::run::DetectRequest;
use grana_pipeline::{
    Batch, BatchEvent, BatchPoll, CaptureFactory, Event, Outcome, Plan as BatchPlan,
};
use grana_protocol::{NamedViewSpec, Protocol, Step, ViewSpec};
use recover::Restore;
use tasks::Tasks;

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
    pub snapshot: bool,
    pub figure: bool,
    pub lineage: bool,
    pub chart: bool,
    pub comparison: bool,
    pub movie: Option<bool>,
    pub sequence: bool,
    pub cancel_export: bool,
    pub keyframe: bool,
    pub repaint: bool,
}

pub struct Session {
    scene: Option<Scene>,
    store: Arc<GeometryStore>,
    measure: Measure,
    tasks: Tasks,
    timeline: Timeline,
    source: Option<Loader>,
    channels: Option<Arc<grana_pipeline::Channels>>,
    analysis: Vec<Analysis>,
    progress: Vec<Progress>,
    in_flight: HashSet<BrickRequest>,
    failed: HashSet<BrickRequest>,
    bricks: Vec<(BrickRequest, BrickData)>,
    pending: Option<Pending>,
    loading: Option<String>,
    message: Option<Message>,
    project: Option<PathBuf>,
    baseline: Option<u64>,
    deferred: bool,
    recovery: Recovery,
    restore: Option<Restore>,
    protocol: Option<Protocol>,
    protocol_text: Option<String>,
    recipe: Option<Recipe>,
    coloc_thresholds: Option<(ColocPair, ColocThresholds)>,
    current_view: Option<ViewState>,
    views: Vec<NamedView>,
    batch: Option<Batch>,
    batch_datasets: Vec<grana_pipeline::DatasetEntry>,
    batch_rows: Vec<BatchDataset>,
    batch_comparison: Vec<BatchComparison>,
    batch_metrics: Vec<BatchMetric>,
    batch_significance: Vec<BatchSignificance>,
    batch_interactions: Vec<BatchInteraction>,
    batch_output: Option<PathBuf>,
    batch_running: bool,
    batch_paused: bool,
    batch_review: Option<(usize, PathBuf, usize, Fingerprint)>,
    batch_review_pending: Option<(usize, PathBuf, usize)>,
    batch_jobs: usize,
    batch_project: bool,
    batch_channels: bool,
    batch_message: Option<String>,
    stamps: Revisions,
    revision: Revision,
    replaced: Revision,
    content: Revision,
    view: Option<ViewState>,
    history: History,
    export: Export,
    labels_queue: Vec<ObjectSetId>,
    label_images: Vec<String>,
    resume: Vec<(ObjectSetId, ObjectSetId, i32)>,
    recut: Option<(ObjectSetId, SurfaceCut)>,
    capturer: Option<CaptureFactory>,
    series: Vec<Series>,
    series_index: usize,
    plate: Option<Plate>,
    field: usize,
    fields: Fields,
    fingerprint: Option<Fingerprint>,
    mask_stamp: Option<(u32, u64)>,
    transfer: Transfer,
    recent: Recent,
    import: Vec<ImportNote>,
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
            tasks: Tasks::default(),
            timeline: Timeline::new(30, 1.0),
            source: None,
            channels: None,
            analysis: Vec::new(),
            progress: Vec::new(),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
            bricks: Vec::new(),
            pending: None,
            loading: None,
            message: None,
            project: None,
            baseline: None,
            deferred: false,
            recovery: Recovery::default(),
            restore: None,
            protocol: None,
            protocol_text: None,
            recipe: None,
            coloc_thresholds: None,
            current_view: None,
            views: Vec::new(),
            batch: None,
            batch_datasets: Vec::new(),
            batch_rows: Vec::new(),
            batch_comparison: Vec::new(),
            batch_metrics: Vec::new(),
            batch_significance: Vec::new(),
            batch_interactions: Vec::new(),
            batch_output: None,
            batch_running: false,
            batch_paused: false,
            batch_review: None,
            batch_review_pending: None,
            batch_jobs: grana_pipeline::default_jobs(),
            batch_project: false,
            batch_channels: false,
            batch_message: None,
            stamps: Revisions::new(),
            revision: Revision::ZERO,
            replaced: Revision::ZERO,
            content: Revision::ZERO,
            view: None,
            history: History::default(),
            export: Export::NATIVE,
            labels_queue: Vec::new(),
            label_images: Vec::new(),
            resume: Vec::new(),
            recut: None,
            capturer: None,
            series: Vec::new(),
            series_index: 0,
            plate: None,
            field: 0,
            fields: Fields::default(),
            fingerprint: None,
            mask_stamp: None,
            transfer: Transfer::default(),
            recent: Recent::default(),
            import: Vec::new(),
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
        self.touch();
        index
    }
    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }
    pub(super) fn analysis_channels(&self) -> Option<Arc<grana_pipeline::Channels>> {
        self.channels.clone()
    }
    pub fn image(&self) -> Option<Arc<grana_image::Image>> {
        self.channels
            .as_ref()
            .map(|channels| channels.image_shared())
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
    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn replaced(&self) -> Revision {
        self.replaced
    }

    pub fn content(&self) -> Revision {
        self.content
    }

    pub fn remote(&self) -> bool {
        self.scene
            .as_ref()
            .is_some_and(|scene| scene.dataset.origin() == Origin::Remote)
    }

    pub fn throughput(&self) -> Option<f32> {
        if self.remote() {
            self.transfer.rate()
        } else {
            None
        }
    }

    pub(crate) fn touch(&mut self) -> Revision {
        self.revision = self.stamps.mint();
        self.revision
    }

    pub(crate) fn replace(&mut self) {
        self.replaced = self.touch();
        self.content = self.replaced;
    }

    pub(crate) fn reloaded(&mut self) {
        self.content = self.touch();
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
        self.scene.is_some() && self.document_hash() != self.baseline
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
            metrics: &self.batch_metrics,
            significance: &self.batch_significance,
            interactions: &self.batch_interactions,
            output: self
                .batch_output
                .as_ref()
                .map(|path| path.display().to_string()),
            jobs: self.batch_jobs,
            project: self.batch_project,
            channels: self.batch_channels,
            running: self.batch_running,
            paused: self.batch_paused,
            review: self.batch_review.as_ref().map(|(index, _, _, _)| *index),
            review_ready: self.review_ready(),
            message: self.batch_message.clone(),
        })
    }
    pub fn series(&self) -> SeriesView<'_> {
        SeriesView {
            entries: &self.series,
            selected: self.series_index,
        }
    }
    pub fn plate(&self) -> Option<PlateView<'_>> {
        Some(PlateView {
            plate: self.plate.as_ref()?,
            selected: self.field,
            thumbs: self.tasks.previews().map(preview::Previews::thumbs),
            previews: self.tasks.previews().map_or(0, preview::Previews::done),
        })
    }
    pub fn label_images(&self) -> &[String] {
        &self.label_images
    }
    pub fn io<'a>(
        &'a self,
        exporting: Option<(u32, u32)>,
        export_message: Option<String>,
    ) -> IoView<'a> {
        let frames = self.scene.as_ref().map_or(0, |scene| scene.frame_count());
        let interval = self
            .scene
            .as_ref()
            .map_or(0.0, |scene| scene.dataset.frame_interval());
        let stats = self.store.stats();
        let document = self.tasks.document();
        IoView {
            loading: self.loading.as_deref(),
            document,
            message: self.message.as_ref(),
            project: self.project_name(),
            dirty: self.dirty(),
            pending: self.in_flight.len(),
            frames,
            frame_interval: interval,
            exporting,
            export_message,
            label: self.write_progress(),
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
    pub fn views(&self) -> &[NamedView] {
        &self.views
    }
    pub fn import(&self) -> &[ImportNote] {
        &self.import
    }
    pub fn recent(&self) -> &[RecentEntry] {
        self.recent.entries()
    }
    pub fn attach_recent_store(&mut self) {
        self.recent = Recent::persistent();
    }
    pub(super) fn record_recent(&mut self, entry: RecentEntry) {
        if let Err(error) = self.recent.record(entry) {
            self.message = Some(Message::info(format!(
                "the list of recent datasets could not be saved: {error}"
            )));
        }
    }
    pub(super) fn save_view(&mut self) -> bool {
        let Some(view) = self.current_view.clone() else {
            self.message = Some(Message::info("open a dataset to save its view"));
            return false;
        };
        let name = self.free_view_name();
        self.views.push(NamedView::new(name, view));
        self.touch();
        true
    }
    pub(super) fn apply_view(&mut self, index: usize) -> bool {
        let Some(named) = self.views.get(index).map(|named| named.view.clone()) else {
            return false;
        };
        self.timeline = named.timeline.clone();
        self.export = named.export;
        self.view = Some(named);
        self.touch();
        true
    }
    pub(super) fn remove_view(&mut self, index: usize) -> bool {
        if index >= self.views.len() {
            return false;
        }
        self.views.remove(index);
        self.touch();
        true
    }
    fn free_view_name(&self) -> String {
        let mut ordinal = self.views.len() + 1;
        loop {
            let name = format!("View {ordinal}");
            if self.views.iter().all(|named| named.name != name) {
                return name;
            }
            ordinal += 1;
        }
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
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        self.measure.set_roi(scene.region_scope());
        if self.tasks.measuring() {
            return;
        }
        if let Some(request) = self.measure.request(scene, busy) {
            self.tasks.start_measure(request);
            return;
        }
        if !self.tasks.plotting() {
            let (low, high) = scene.region.bounds();
            let window = grana_measure::Window::new(low.to_array(), high.to_array());
            if let Some(request) = self.measure.plot_request(window) {
                self.tasks.start_plot(request);
            }
        }
    }
    pub fn settle_measure(&mut self) -> bool {
        let Some(slot) = self.tasks.measure.as_mut() else {
            return false;
        };
        let key = slot.key();
        match slot.poll() {
            Poll::Event(table) => {
                self.tasks.measure = None;
                self.finish_measure(key, table);
                true
            }
            Poll::Empty => false,
            Poll::Closed => {
                self.tasks.measure = None;
                self.finish_measure(key, None);
                true
            }
        }
    }
    pub fn apply_measure(&mut self) {
        if self.measure.take_dirty() {
            self.touch();
            if let Some(scene) = self.scene.as_mut() {
                self.measure.apply(scene);
            }
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut repaint = self.poll_source();
        let (completions, stepped) = self.tasks.poll();
        for completion in completions {
            self.finish(completion);
        }
        repaint |= stepped;
        repaint |= self.poll_analysis();
        repaint |= self.poll_batch();
        repaint |= self.refresh_masks();
        if self.deferred && self.analysis.is_empty() && self.progress.is_empty() {
            self.baseline = self.document_hash();
            self.deferred = false;
        }
        if repaint {
            self.touch();
        }
        repaint
    }

    pub fn previewing(&self) -> bool {
        self.tasks.previewing()
    }

    pub fn writing(&self) -> bool {
        self.tasks.writing()
    }
    pub fn analysing(&self) -> bool {
        !self.analysis.is_empty() || self.writing()
    }
    pub fn measuring(&self) -> bool {
        self.tasks.measuring()
    }
    pub fn plotting(&self) -> bool {
        self.tasks.plotting()
    }
    pub fn write_progress(&self) -> Option<(&str, usize, usize)> {
        self.tasks.progress()
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
            if request.time < time || self.in_flight.contains(request) {
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
        self.touch();
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
