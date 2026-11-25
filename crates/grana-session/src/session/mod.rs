mod analysis;
mod batch;
mod dataset;
mod derived;
mod edit;
mod execute;
mod export;
mod field;
mod import;
mod input;
mod intensity;
mod interop;
mod label;
mod localization;
mod mask;
mod montage;
mod open;
mod package;
mod plate;
mod preview;
mod provenance;
mod recover;
mod replay;
mod scene_import;
mod seed;
mod stitch;
mod tasks;
use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_animation::{Keyframe, Timeline};
use grana_display::Shading;
use grana_image::Format;
use grana_image::Plate;
use grana_measure::{ColocPair, ColocThresholds, Group, Scope};
use grana_object::SurfaceCut;
use grana_object::TrackSet;
use grana_optics::Psf;
use grana_project::{Mismatch, NamedView, Project, Source as ProjectSource, ViewState};
use grana_scene::{
    Criteria, Dataset, FilterRange, Item, Job, LassoPath, MaskExtent, Measure, ObjectFrame,
    ObjectParams, ObjectSetId, Origin, Progress, Revision, Revisions, Scene,
};
use grana_store::SPILL_BUDGET;
use grana_store::SpillStore;
use grana_volume::{
    BrickRequest, Calibration, Damage, Fingerprint, PackedBrick, Spacing, packed_bytes,
};

use crate::access::Access;
use crate::command::{Edit, Export};
use crate::document::{self, Document};
use crate::history::{History, Snapshot, Undo};
use crate::input::Input;
use crate::loader::{Event as LoaderEvent, Loader};
use crate::network::Watcher;
use crate::rate::Rate;
use crate::reads::Reads;
use crate::recent::{Entry as RecentEntry, Recent};
use crate::recipe::Recipe;
use crate::recovery::Recovery;
use crate::task::Poll;
use crate::view::{
    BatchComparison, BatchDataset, BatchInteraction, BatchMetric, BatchSignificance, BatchView,
    Held, HistoryView, ImportNote, IoView, Message, PlateView, SeriesView, StoreView, Transfer,
};
use crate::{Action, Analysis, Command, Inquiry, Series, Source};
use edit::{Edits, Follow, Rollback};
use field::{Fields, Recovered};
use grana_pipeline::run::DetectRequest;
use grana_pipeline::{
    Batch, BatchEvent, BatchPoll, CaptureFactory, Event, Outcome, Plan as BatchPlan,
};
use grana_protocol::{NamedViewSpec, Protocol, Staged, Step, ViewSpec};
pub use localization::Localizations;
use recover::{Restore, Restored};
use stitch::Stitch;
use tasks::Tasks;

const MAX_IN_FLIGHT: usize = 8192;

const READ_BYTES: u64 = 256 << 20;

const DRAIN: Duration = Duration::from_millis(2);

const RETRY_FLOOR: Duration = Duration::from_millis(250);

const RETRY_CEILING: Duration = Duration::from_secs(8);

const RETRY_LIMIT: u32 = 6;

const DECODE_REPORT: Duration = Duration::from_secs(1);

struct Failure {
    attempts: u32,
    retry_at: Instant,
}

impl Failure {
    fn backoff(attempts: u32) -> Duration {
        RETRY_FLOOR
            .saturating_mul(1 << attempts.min(5))
            .min(RETRY_CEILING)
    }
}

struct Pending {
    path: PathBuf,
    project: Project,
}

type Answer = Box<dyn FnOnce(&mut Session, Option<PathBuf>)>;

#[derive(Default)]
pub struct Effects {
    pub open_any: bool,
    pub open_dataset: bool,
    pub open_image: bool,
    pub open_tiles: bool,
    pub open_project: bool,
    pub open_package: bool,
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
    pub held: Vec<Held>,
}

pub struct Session {
    access: Access,
    read_only: bool,
    scene: Option<Scene>,
    store: Arc<SpillStore>,
    measure: Measure,
    tasks: Tasks,
    timeline: Timeline,
    source: Option<Loader>,
    channels: Option<Arc<grana_pipeline::Channels>>,
    analysis: Vec<Analysis>,
    progress: Vec<Progress>,
    in_flight: HashSet<BrickRequest>,
    plan: u64,
    abandoned: Vec<BrickRequest>,
    reads_started: u64,
    reads_delivered: u64,
    decode_reported: Option<Instant>,
    failed: HashMap<BrickRequest, Failure>,
    bricks: Vec<(BrickRequest, PackedBrick)>,
    pending: Option<Pending>,
    inquiry: Option<Inquiry>,
    answer: Option<Answer>,
    loading: Option<String>,
    message: Option<Message>,
    project: Option<PathBuf>,
    baseline: Option<u64>,
    hash: Cell<Option<(Revision, u64)>>,
    deferred: bool,
    recovery: Recovery,
    restore: Option<Restore>,
    protocol: Option<Protocol>,
    protocol_text: Option<String>,
    captured: Option<replay::CapturedProtocol>,
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
    labels_failed: Vec<String>,
    edits: Edits,
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
    mask_stamps: Vec<u64>,
    mask_extents: Vec<Option<MaskExtent>>,
    damages: Vec<Damage>,
    transfer: Rate,
    network: Watcher,
    recent: Recent,
    import: Vec<ImportNote>,
    stitch: Option<Stitch>,
    localization: Localizations,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self::with_access(Access::Author)
    }

    pub fn viewer() -> Self {
        Self::with_access(Access::Viewer)
    }

    fn with_access(access: Access) -> Self {
        Self {
            access,
            read_only: false,
            scene: None,
            store: SpillStore::temporary(SPILL_BUDGET)
                .expect("the geometry store needs a temporary file"),
            measure: Measure::new(),
            tasks: Tasks::default(),
            timeline: Timeline::new(30, 1.0),
            source: None,
            channels: None,
            analysis: Vec::new(),
            progress: Vec::new(),
            in_flight: HashSet::new(),
            plan: 0,
            abandoned: Vec::new(),
            reads_started: 0,
            reads_delivered: 0,
            decode_reported: None,
            failed: HashMap::new(),
            bricks: Vec::new(),
            pending: None,
            inquiry: None,
            answer: None,
            loading: None,
            message: None,
            project: None,
            baseline: None,
            hash: Cell::new(None),
            deferred: false,
            recovery: Recovery::default(),
            restore: None,
            protocol: None,
            protocol_text: None,
            captured: None,
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
            labels_failed: Vec::new(),
            edits: Edits::default(),
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
            mask_stamps: Vec::new(),
            mask_extents: Vec::new(),
            damages: Vec::new(),
            transfer: Rate::default(),
            network: Watcher::new(),
            recent: Recent::default(),
            import: Vec::new(),
            stitch: None,
            localization: Localizations::default(),
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
            self.history.record(
                Edit::new(Action::Keyframe),
                Undo::Snapshot(Box::new(snapshot)),
            );
        }
        self.touch();
        index
    }
    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }
    pub fn access(&self) -> Access {
        if self.read_only {
            Access::Viewer
        } else {
            self.access
        }
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
        self.sync_localizations();
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

    pub(crate) fn discard_streamed(&mut self) {
        if let Some(source) = self.source.as_ref() {
            source.invalidate();
        }
        self.in_flight.clear();
        self.abandoned.clear();
        self.failed.clear();
        self.bricks.clear();
    }

    pub(crate) fn reload_streamed(&mut self) {
        self.discard_streamed();
        self.reloaded();
    }

    pub fn pending(&self) -> usize {
        self.in_flight.len()
    }

    pub fn pending_requests(&self) -> &HashSet<BrickRequest> {
        &self.in_flight
    }

    pub fn working(&self) -> bool {
        self.loading.is_some() || !self.in_flight.is_empty() || self.running()
    }

    pub fn running(&self) -> bool {
        !self.analysis.is_empty()
            || !self.progress.is_empty()
            || self.tasks.busy()
            || self.edits.busy()
            || self.stitch.as_ref().is_some_and(Stitch::busy)
            || self.batch_running
            || self.previewing()
    }

    pub(super) fn ask(
        &mut self,
        inquiry: Inquiry,
        answer: impl FnOnce(&mut Session, Option<PathBuf>) + 'static,
    ) {
        assert!(
            self.inquiry.is_none() && self.answer.is_none(),
            "a session asks one inquiry at a time"
        );
        self.inquiry = Some(inquiry);
        self.answer = Some(Box::new(answer));
    }

    pub fn take_inquiry(&mut self) -> Option<Inquiry> {
        self.inquiry.take()
    }

    pub fn inquiry_pending(&self) -> bool {
        self.inquiry.is_some()
    }

    pub fn answer_inquiry(&mut self, path: Option<PathBuf>) {
        self.inquiry = None;
        let Some(answer) = self.answer.take() else {
            self.message = Some(Message::error(
                "the answered inquiry no longer belongs to the session",
            ));
            return;
        };
        answer(self, path);
    }
    pub fn loading(&self) -> Option<&str> {
        self.loading.as_deref()
    }
    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }
    pub fn document_name(&self) -> Option<String> {
        self.project
            .as_ref()
            .map(|path| file_name(path))
            .or_else(|| {
                self.scene
                    .as_ref()
                    .map(|scene| scene.dataset.name().to_string())
            })
    }
    pub fn dirty(&self) -> bool {
        if self.scene.is_none() {
            return false;
        }
        let hash = match self.hash.get() {
            Some((revision, hash)) if revision == self.revision => hash,
            _ => {
                let hash = self
                    .document_hash()
                    .expect("a loaded document hashes its content");
                self.hash.set(Some((self.revision, hash)));
                hash
            }
        };
        self.baseline != Some(hash)
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
        failure: Option<&'a str>,
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
            failure,
            name: self.document_name(),
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
            network: self.network.view(),
        }
    }
    pub fn restore_timeline(&mut self, timeline: Timeline) {
        let frames = self.scene.as_ref().map_or(1, |scene| scene.frame_count());
        self.timeline = timeline.clamped(frames);
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
            self.message = Some(Message::error("the document holds no such named view"));
            return false;
        };
        self.restore_timeline(named.timeline);
        self.export = named.export;
        self.view = Some(ViewState {
            camera: named.camera,
            timeline: self.timeline.clone(),
            export: self.export,
        });
        self.touch();
        true
    }
    pub(super) fn remove_view(&mut self, index: usize) -> bool {
        if index >= self.views.len() {
            self.message = Some(Message::error("the document holds no such named view"));
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
    pub fn take_bricks(&mut self) -> Vec<(BrickRequest, PackedBrick)> {
        let bricks = std::mem::take(&mut self.bricks);
        for (request, _) in &bricks {
            self.in_flight.remove(request);
        }
        if !bricks.is_empty() && self.in_flight.is_empty() {
            self.report_decode();
        }
        bricks
    }

    fn report_decode(&mut self) {
        let now = Instant::now();
        if self
            .decode_reported
            .is_some_and(|held| now.duration_since(held) < DECODE_REPORT)
        {
            return;
        }
        self.decode_reported = Some(now);
        if let Some(decode) = self
            .source
            .as_ref()
            .and_then(Loader::channels)
            .and_then(|channels| channels.decode())
        {
            grana_log::info!(
                target: "grana::stream",
                hits = decode.hits(),
                misses = decode.misses(),
                hit_rate = decode.hit_rate(),
                amplification = decode.amplification(),
                "the decode cache reports its reuse"
            );
        }
        report_pools();
    }
    pub fn take_abandoned(&mut self) -> Vec<BrickRequest> {
        std::mem::take(&mut self.abandoned)
    }
    pub fn reads(&self) -> Reads {
        Reads {
            started: self.reads_started,
            delivered: self.reads_delivered,
        }
    }
    pub fn take_damage(&mut self) -> Vec<Damage> {
        std::mem::take(&mut self.damages)
    }
    fn sync_localizations(&mut self) -> bool {
        match self.scene.as_mut() {
            Some(scene) => self.localization.sync(scene),
            None => self.localization.clear(),
        }
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
        if !self.tasks.listing()
            && let Some(request) = self.measure.rows_request()
        {
            self.tasks.start_rows(request);
            return;
        }
        if !self.tasks.plotting() {
            let (low, high) = scene.measure_bounds();
            let window = grana_measure::Window::new(low.to_array(), high.to_array());
            if let Some(request) = self.measure.plot_request(window) {
                self.tasks.start_plot(request);
            }
        }
        if !self.tasks.materializing()
            && let Some(request) = self.measure.materialize_request()
        {
            self.tasks.start_materialize(request);
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
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if self.measure.apply(scene) {
            self.touch();
        }
    }
    pub fn poll(&mut self) -> bool {
        self.network.sample(Instant::now());
        let mut repaint = self.poll_source();
        let (completions, stepped) = self.tasks.poll();
        for completion in completions {
            self.finish(completion);
        }
        repaint |= stepped;
        repaint |= self.poll_analysis();
        repaint |= self.poll_edits();
        repaint |= self.poll_batch_scan();
        repaint |= self.poll_batch();
        repaint |= self.poll_stitch();
        repaint |= self.refresh_masks();
        repaint |= self.sync_localizations();
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

    pub fn listing(&self) -> bool {
        self.tasks.listing()
    }
    pub fn materializing(&self) -> bool {
        self.tasks.materializing()
    }
    pub fn plotting(&self) -> bool {
        self.tasks.plotting()
    }
    pub fn deriving(&self) -> bool {
        self.tasks.deriving()
    }
    pub fn write_progress(&self) -> Option<(&str, usize, usize)> {
        self.tasks.progress()
    }
    pub fn request(
        &mut self,
        time: u32,
        plan: u64,
        demand: &[BrickRequest],
        prefetch: &[BrickRequest],
    ) {
        if self.loading.is_some() {
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            return;
        };
        let count = channels.pyramid().channel_count();
        let now = Instant::now();
        let Self {
            source,
            in_flight,
            plan: plan_state,
            failed,
            message,
            reads_started,
            ..
        } = self;
        let Some(loader) = source.as_ref() else {
            return;
        };
        loader.set_plan(plan);
        if plan != *plan_state {
            *plan_state = plan;
            in_flight.clear();
        }
        let bytes = |request: &BrickRequest| packed_bytes(count, request.channels);
        let mut used = in_flight.iter().map(bytes).sum::<u64>();
        let mut held = in_flight.len();
        let mut demand_batch = Vec::with_capacity(demand.len().min(MAX_IN_FLIGHT));
        let mut prefetch_batch = Vec::with_capacity(prefetch.len().min(MAX_IN_FLIGHT));
        for (bricks, batch) in [(demand, &mut demand_batch), (prefetch, &mut prefetch_batch)] {
            for request in bricks {
                if request.time < time || in_flight.contains(request) {
                    continue;
                }
                if let Some(failure) = failed.get(request)
                    && (failure.attempts >= RETRY_LIMIT || now < failure.retry_at)
                {
                    continue;
                }
                let cost = bytes(request);
                if held >= MAX_IN_FLIGHT || used.saturating_add(cost) > READ_BYTES {
                    continue;
                }
                held += 1;
                used += cost;
                batch.push(*request);
            }
        }
        if demand_batch.is_empty() && prefetch_batch.is_empty() {
            return;
        }
        in_flight.extend(demand_batch.iter().copied());
        in_flight.extend(prefetch_batch.iter().copied());
        *reads_started += (demand_batch.len() + prefetch_batch.len()) as u64;
        if !loader.request(&demand_batch, &prefetch_batch) {
            *source = None;
            in_flight.clear();
            *message = Some(Message::error("the dataset reader stopped"));
        }
    }

    pub fn retry_wait(&self) -> Option<Duration> {
        let now = Instant::now();
        self.failed
            .values()
            .map(|failure| failure.retry_at.saturating_duration_since(now))
            .min()
    }
    pub fn set_time(&mut self, time: u32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if time == scene.time() {
            return;
        }
        scene.set_time(time);
        self.mask_extents = scene.mask_extents(time);
        self.in_flight.retain(|request| request.time >= time);
        self.failed.retain(|request, _| request.time >= time);
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

fn report_pools() {
    for pool in grana_store::Pool::all() {
        let meter = pool.meter();
        if meter.hits() + meter.misses() == 0 {
            continue;
        }
        grana_log::info!(
            target: "grana::cache",
            pool = pool.name(),
            share = pool.share(),
            hits = meter.hits(),
            misses = meter.misses(),
            hit_rate = meter.hit_rate(),
            evicted = meter.evicted(),
            "the cache pool reports its reuse"
        );
    }
}
