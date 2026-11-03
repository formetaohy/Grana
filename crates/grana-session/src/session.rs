use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_animation::{Keyframe, Timeline};
use grana_display::Shading;
use grana_geometry::{DEFAULT_BUDGET, MeshStore};
use grana_image::Format;
use grana_measure::{ColocPair, ColocThresholds, Group};
use grana_object::TrackSet;
use grana_project::{Project, Source as ProjectSource, ViewState};
use grana_scene::{Dataset, Item, Job, Measure, ObjectFrame, ObjectSetId, Origin, Progress, Scene};
use grana_volume::{BrickData, BrickRequest, Region};

use crate::command::{Edit, Export};
use crate::document::{self, Document};
use crate::history::{History, Snapshot};
use crate::loader::{Event as LoaderEvent, Loader};
use crate::protocol::Step;
use crate::recipe::Recipe;
use crate::run::DetectRequest;
use crate::sample;
use crate::task::Poll;
use crate::view::{BatchDataset, BatchView, HistoryView, IoView, Message, StoreView};
use crate::{
    Action, Analysis, Batch, BatchEvent, BatchPoll, Command, Event, Outcome, Plan as BatchPlan,
    Protocol, Source, ViewSpec,
};

const MAX_IN_FLIGHT: usize = 8192;
const UPLOAD_BATCH: usize = 64;

struct Pending {
    path: PathBuf,
    project: Project,
}

struct Recording {
    edit: Edit,
    snapshot: Snapshot,
    hash: u64,
    analyses: usize,
}

enum Next {
    Detect {
        set: ObjectSetId,
        mask: Option<ObjectSetId>,
    },
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
    },
    Coloc {
        pair: ColocPair,
        thresholds: ColocThresholds,
        mask: Option<ObjectSetId>,
    },
    Present,
}

#[derive(Default)]
pub struct Effects {
    pub open_dataset: bool,
    pub open_image: bool,
    pub open_project: bool,
    pub save_project: Option<bool>,
    pub snapshot: Option<Export>,
    pub export_movie: Option<u32>,
    pub sequence: Option<Export>,
    pub cancel_export: bool,
    pub keyframe: bool,
    pub repaint: bool,
}

pub struct Session {
    scene: Option<Scene>,
    store: Arc<MeshStore>,
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
    batch_datasets: Vec<PathBuf>,
    batch_rows: Vec<BatchDataset>,
    batch_output: Option<PathBuf>,
    batch_running: bool,
    batch_jobs: usize,
    batch_project: bool,
    batch_message: Option<String>,
    generation: u64,
    view: Option<ViewState>,
    history: History,
    export: Export,
    resume: Vec<(ObjectSetId, ObjectSetId, i32)>,
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
            store: MeshStore::temporary(DEFAULT_BUDGET)
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
            batch_output: None,
            batch_running: false,
            batch_jobs: crate::default_jobs(),
            batch_project: false,
            batch_message: None,
            generation: 0,
            view: None,
            history: History::default(),
            export: Export::NATIVE,
            resume: Vec::new(),
        }
    }

    pub fn open_series(&mut self, name: &str, series: grana_volume::VolumeSeries) {
        let (source, pyramid) = Loader::memory(series);
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
        self.in_flight.clear();
        self.bricks.clear();
        self.measure.reset();
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

    pub fn set_protocol(&mut self, protocol: Protocol) {
        self.protocol_text = Some(protocol.summary());
        self.protocol = Some(protocol);
        self.reset_batch_rows();
        self.batch_message = None;
    }

    pub fn replaying(&self) -> bool {
        self.recipe.is_some()
    }

    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }

    pub fn measure(&self) -> &Measure {
        &self.measure
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
            output: self
                .batch_output
                .as_ref()
                .map(|path| path.display().to_string()),
            jobs: self.batch_jobs,
            project: self.batch_project,
            running: self.batch_running,
            message: self.batch_message.clone(),
        })
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

    pub fn delete_selected(&mut self) -> bool {
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        let Some(selection) = scene.selection() else {
            return false;
        };
        let Item::Object(object) = selection.item else {
            return false;
        };
        let time = scene.time();
        self.execute(Command::DeleteObject {
            set: selection.set,
            time,
            object,
        });
        true
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

    pub fn execute(&mut self, command: Command) -> Effects {
        match command {
            Command::Undo => return self.undo(),
            Command::Redo => return self.redo(),
            _ => {}
        }
        let recording = self.begin_edit(&command);
        let mut effects = Effects::default();
        match command {
            Command::OpenDataset => effects.open_dataset = true,
            Command::OpenImage => effects.open_image = true,
            Command::OpenDemo => self.open_sample(),
            Command::OpenProject => effects.open_project = true,
            Command::SaveProject => effects.save_project = Some(false),
            Command::SaveProjectAs => effects.save_project = Some(true),
            Command::SaveSnapshot => effects.snapshot = Some(self.export),
            Command::SetTime(time) => self.set_time(time),
            Command::Select(selection) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.select(selection);
                }
            }
            Command::AddObject(kind) => {
                if let Some(scene) = self.scene.as_mut() {
                    let channel = scene.selected;
                    scene.add_object(kind, channel);
                }
            }
            Command::RenameObject { set, name } => {
                let renamed = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.rename_object(set, &name));
                self.message = (!renamed).then(|| {
                    Message::error("object set names differ and do not collide as file names")
                });
            }
            Command::ReorderObject { set, index } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.reorder_object(set, index);
                }
            }
            Command::DeleteObject { set, time, object } => {
                let removed = self
                    .scene
                    .as_mut()
                    .map(|scene| scene.delete_object(set, time, object));
                if let Some(Err(detail)) = removed {
                    self.message = Some(Message::error(detail));
                }
            }
            Command::RemoveObject(set) => self.remove_object(set),
            Command::Detect(set) => self.start_detect(set),
            Command::CancelDetect(set) => self.cancel(Job::Detect(set)),
            Command::ClearObject(set) => {
                self.cancel(Job::Detect(set));
                self.cancel(Job::Track(set));
                self.clear_relations(set);
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_object(set);
                }
            }
            Command::AddSeed { set, position } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.add_seed(set, position);
                }
            }
            Command::RemoveSeed { set, index } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.remove_seed(set, index);
                }
            }
            Command::ClearSeeds(set) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_seeds(set);
                }
            }
            Command::AddRelation {
                source,
                target,
                shift,
            } => self.add_relation(source, target, shift),
            Command::SetRelationShift {
                source,
                target,
                shift,
            } => self.set_relation_shift(source, target, shift),
            Command::RemoveRelation { source, target } => self.remove_relation(source, target),
            Command::Track(set) => self.start_tracks(set),
            Command::CancelTrack(set) => self.cancel(Job::Track(set)),
            Command::ClearTracks(set) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_tracks(set);
                }
            }
            Command::SelectChannel(index) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.select_channel(index);
                }
            }
            Command::SetChannelEnabled { channel, enabled } => {
                if let Some(scene) = self.scene.as_mut()
                    && let Some(target) = scene.channels.get_mut(channel)
                {
                    target.enabled = enabled;
                }
            }
            Command::SetChannelOpacity { channel, opacity } => {
                if let Some(scene) = self.scene.as_mut()
                    && let Some(target) = scene.channels.get_mut(channel)
                {
                    target.opacity = opacity.clamp(0.0, 1.0);
                }
            }
            Command::SetWindow { channel, window } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_window(channel, window.min(), window.max());
                }
            }
            Command::ResetWindow(channel) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.reset_window(channel);
                }
            }
            Command::SetTransfer { channel, transfer } => {
                if let Some(scene) = self.scene.as_mut()
                    && let Some(target) = scene.channels.get_mut(channel)
                {
                    target.set_transfer(transfer);
                }
            }
            Command::SetObjectVisible { set, visible } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_visible(visible);
                }
            }
            Command::SetObjectColor { set, color } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_color(color);
                }
            }
            Command::SetObjectOpacity { set, opacity } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_opacity(opacity);
                }
            }
            Command::SetObjectParams { set, params } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_params(params);
                }
            }
            Command::SetObjectColoring { set, coloring } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_object_coloring(coloring);
                }
            }
            Command::SetMask(source) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask(source);
                }
            }
            Command::SetTrackParams { set, params } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_track_params(params);
                }
            }
            Command::SetTrackVisible { set, visible } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_track_visible(visible);
                }
            }
            Command::SetTrackThickness { set, thickness } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_track_thickness(set, thickness);
                }
            }
            Command::SetTrackColoring { set, coloring } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_track_coloring(coloring);
                }
            }
            Command::SelectTrack { set, index } => {
                if let Some(index) = index
                    && let Some(scene) = self.scene.as_mut()
                {
                    scene.select_track(set, index);
                }
            }
            Command::CutTrack { set, time, object } => {
                let cut = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.cut_track(set, time, object));
                self.message = (!cut)
                    .then(|| Message::error("the track ends at this frame and cannot be cut"));
            }
            Command::JoinTracks {
                set,
                source,
                target,
            } => {
                let joined = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.join_tracks(set, source, target));
                self.message = (!joined)
                    .then(|| Message::error("a track joins a later track that starts a lineage"));
            }
            Command::SetRegion(region) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_region(region);
                }
            }
            Command::SetExposure(exposure) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.exposure = exposure;
                }
            }
            Command::SetStepScale(step) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.step_scale = step;
                }
            }
            Command::SetShading(shading) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.shading = shading;
                }
            }
            Command::ResetRendering => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.shading = Shading::default();
                    scene.exposure = 1.0;
                    scene.step_scale = 1.0;
                }
            }
            Command::SetMeasureSet(set) => {
                if let (Some(set), Some(scene)) = (set, self.scene.as_mut()) {
                    self.measure.select(set, scene);
                }
            }
            Command::SetMeasureGroup(group) => {
                if let Some(scene) = self.scene.as_mut() {
                    self.measure.set_group(group, scene);
                }
            }
            Command::SetMeasureAggregate(aggregate) => {
                if let Some(scene) = self.scene.as_mut() {
                    self.measure.set_aggregate(aggregate, scene);
                }
            }
            Command::SetMeasureScope(scope) => self.measure.set_scope(scope),
            Command::SetLasso(lasso) => self.measure.set_lasso(lasso),
            Command::AddFilter => self.measure.add_filter(),
            Command::RemoveFilter(index) => self.measure.remove_filter(index),
            Command::SetFilter { index, filter } => self.measure.set_filter(index, filter),
            Command::RetargetFilter { index, column } => {
                self.measure.retarget_filter(index, column);
            }
            Command::SetColumnVisible { index, visible } => {
                self.measure.set_column_visible(index, visible);
            }
            Command::ShowAllColumns => self.measure.show_all_columns(),
            Command::SetMeasureVisibility(visible) => self.measure.set_visibility(visible),
            Command::SetMeasureView(view) => self.measure.set_view(view),
            Command::SetPlotAxis { axis, column } => self.measure.set_axis(axis, column),
            Command::SetGate {
                column,
                minimum,
                maximum,
            } => self.measure.set_gate(column, minimum, maximum),
            Command::ToggleSort(index) => self.measure.toggle_sort(index),
            Command::SetColocPair(pair) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.set_pair(pair);
                }
            }
            Command::SetColocThresholds(thresholds) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.set_thresholds(thresholds);
                }
            }
            Command::SetColocSettings(settings) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.set_settings(settings);
                }
            }
            Command::RemoveColocResult(pair) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.remove_result(pair);
                }
            }
            Command::ComputeColoc => self.start_coloc(),
            Command::CancelColoc => self.cancel(Job::Coloc),
            Command::ExportColoc => self.export_coloc(),
            Command::ExportMeasure => self.export_measure(),
            Command::CaptureProtocol => self.capture_protocol(),
            Command::ApplyProtocol => self.apply_protocol(),
            Command::LoadProtocol => self.load_protocol(),
            Command::SaveProtocol => self.save_protocol(),
            Command::AddBatchDatasets => self.add_batch_datasets(),
            Command::ClearBatchDatasets => {
                self.batch_datasets.clear();
                self.batch_rows.clear();
            }
            Command::ChooseBatchOutput => self.choose_batch_output(),
            Command::SetBatchJobs(jobs) => self.set_batch_jobs(jobs),
            Command::SetBatchProject(project) => {
                self.batch_project = project;
                self.reset_batch_rows();
            }
            Command::RunBatch => self.run_batch(),
            Command::CancelBatch => {
                if let Some(batch) = &self.batch {
                    batch.cancel();
                }
            }
            Command::SetKeyframe => effects.keyframe = true,
            Command::ClearKeyframes => self.timeline.clear(),
            Command::DeleteKeyframe(index) => {
                self.timeline.remove(index);
            }
            Command::SetTimelineFps(fps) => self.timeline.set_fps(fps),
            Command::SetTimelineDuration(duration) => self.timeline.set_duration(duration),
            Command::ExportMovie { width } => effects.export_movie = Some(width),
            Command::SetExport(export) => self.export = export,
            Command::ExportSequence(export) => {
                self.export = export;
                effects.sequence = Some(export);
            }
            Command::CancelExport => effects.cancel_export = true,
            Command::Undo | Command::Redo => {
                unreachable!("undo and redo run before the document commands")
            }
        }
        if let Some(recording) = recording {
            self.commit_edit(recording);
        }
        effects
    }

    fn begin_edit(&self, command: &Command) -> Option<Recording> {
        if self.loading.is_some() {
            return None;
        }
        let edit = command.edit()?;
        let snapshot = self.snapshot()?;
        Some(Recording {
            edit,
            snapshot,
            hash: self.document_hash()?,
            analyses: self.analysis.len(),
        })
    }

    fn commit_edit(&mut self, recording: Recording) {
        let Recording {
            edit,
            snapshot,
            hash,
            analyses,
        } = recording;
        let changed = self.document_hash() != Some(hash);
        let started = self.analysis.len() > analyses;
        if changed || (edit.asynchronous() && started) {
            self.history.record(edit, snapshot);
        }
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let scene = self.scene.as_ref()?;
        Some(Snapshot::capture(scene, &self.measure, &self.timeline))
    }

    fn undo(&mut self) -> Effects {
        if self.loading.is_some() {
            return Effects::default();
        }
        let Some(current) = self.snapshot() else {
            return Effects::default();
        };
        let Some((edit, snapshot)) = self.history.undo(current) else {
            return Effects::default();
        };
        self.abandon(edit);
        self.restore(snapshot);
        Effects {
            repaint: true,
            ..Effects::default()
        }
    }

    fn redo(&mut self) -> Effects {
        if self.loading.is_some() {
            return Effects::default();
        }
        let Some(current) = self.snapshot() else {
            return Effects::default();
        };
        let Some((edit, snapshot)) = self.history.redo(current) else {
            return Effects::default();
        };
        self.abandon(edit);
        self.restore(snapshot);
        Effects {
            repaint: true,
            ..Effects::default()
        }
    }

    fn abandon(&mut self, edit: Edit) {
        if edit.action() == Action::ApplyProtocol {
            self.abort_recipe();
            self.analysis.clear();
            self.progress.clear();
            self.resume.clear();
        }
        if let Some(set) = ObjectSetId::from_raw(edit.target()) {
            match edit.action() {
                Action::Detect => {
                    self.cancel(Job::Detect(set));
                    self.cancel(Job::Track(set));
                }
                Action::Track => self.cancel(Job::Track(set)),
                Action::Relation => {
                    self.analysis.retain(|analysis| {
                        !matches!(analysis.job(), Job::Relation { target, .. } if target == set)
                    });
                    self.progress.retain(|progress| {
                        !matches!(progress.job, Job::Relation { target, .. } if target == set)
                    });
                }
                _ => {}
            }
        }
        if edit.action() == Action::ComputeColoc {
            self.cancel(Job::Coloc);
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        let (state, measure, timeline) = snapshot.into_parts();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        scene.restore(state);
        self.measure.restore(measure, scene);
        self.timeline = timeline;
        if let Some(source) = &self.source {
            source.set_time(scene.time());
        }
        self.message = None;
    }

    fn object_mut(&mut self, set: ObjectSetId) -> Option<&mut grana_scene::ObjectSet> {
        self.scene.as_mut()?.objects.get_mut(set)
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

    fn reject_transfer(&mut self) -> bool {
        if self.loading.is_some() {
            return true;
        }
        if self.document.is_some() {
            self.message = Some(Message::error("a project transfer is already running"));
            return true;
        }
        false
    }

    fn open_project(&mut self, path: &Path, project: Project) {
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
            ProjectSource::File { path: data, .. } => {
                let data = data.clone();
                if !data.is_dir() && !data.is_file() {
                    self.message = Some(Message::error(format!(
                        "{} is not a readable image",
                        data.display()
                    )));
                    return;
                }
                self.source = Some(Loader::file(data.clone()));
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

    fn poll_document(&mut self) -> bool {
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

    fn source_ref(&self) -> Option<ProjectSource> {
        let scene = self.scene.as_ref()?;
        let name = scene.dataset.name().to_string();
        match scene.dataset.origin() {
            Origin::Synthetic => Some(ProjectSource::Synthetic { name }),
            Origin::Zarr | Origin::Tiff | Origin::Czi => match self.source.as_ref()?.source() {
                Source::File(path) => Some(ProjectSource::File { name, path }),
                Source::Memory(_) => None,
            },
        }
    }

    pub fn document_hash(&self) -> Option<u64> {
        let scene = self.scene.as_ref()?;
        Some(scene.document_hash() ^ self.measure.document_hash() ^ self.timeline.document())
    }

    fn reset(&mut self) {
        self.measure.reset();
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.message = None;
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.timeline = Timeline::new(30, 1.0);
        self.view = None;
        self.history.clear();
    }

    fn open_dataset(&mut self, dataset: Dataset) {
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
        if let Some(source) = &self.source {
            source.set_time(0);
        }
    }

    fn frame_duration(&self) -> f32 {
        self.scene
            .as_ref()
            .map(|scene| {
                (scene.dataset.frame_count() as f32 * scene.dataset.frame_interval())
                    .clamp(1.0, Timeline::MAX_DURATION)
            })
            .unwrap_or(1.0)
    }

    fn poll_source(&mut self) -> bool {
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
                } => {
                    let dataset = match format {
                        Format::Zarr => Dataset::zarr(name, pyramid),
                        Format::Tiff => Dataset::tiff(name, pyramid),
                        Format::Czi => Dataset::czi(name, pyramid),
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

    fn poll_analysis(&mut self) -> bool {
        enum Update {
            Progress(u32, usize, usize),
            Frame(u32, ObjectFrame),
            Tracks(TrackSet),
            Relation(u32, grana_object::RelationFrame),
            Coloc(Box<grana_measure::ColocSeries>, Region),
            Done,
            Failed(String),
        }
        let mut repaint = false;
        let mut updates: Vec<(usize, Job, Update)> = Vec::new();
        for (index, analysis) in self.analysis.iter().enumerate() {
            let job = analysis.job();
            loop {
                match analysis.poll() {
                    Poll::Event(Event::Progress { time, done, total }) => {
                        updates.push((index, job, Update::Progress(time, done, total)));
                    }
                    Poll::Event(Event::Frame { time, frame }) => {
                        updates.push((index, job, Update::Frame(time, frame)));
                    }
                    Poll::Event(Event::Tracks(set)) => {
                        updates.push((index, job, Update::Tracks(set)));
                    }
                    Poll::Event(Event::Relation { time, frame }) => {
                        updates.push((index, job, Update::Relation(time, frame)));
                    }
                    Poll::Event(Event::Coloc { series, region }) => {
                        updates.push((index, job, Update::Coloc(series, region)));
                    }
                    Poll::Event(Event::Done) => {
                        updates.push((index, job, Update::Done));
                        break;
                    }
                    Poll::Event(Event::Failed(detail)) => {
                        updates.push((index, job, Update::Failed(detail)));
                        break;
                    }
                    Poll::Empty => break,
                    Poll::Closed => {
                        updates.push((
                            index,
                            job,
                            Update::Failed("the analysis worker stopped".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
        let mut removed = Vec::new();
        let mut detected = Vec::new();
        let mut finished = Vec::new();
        for (index, job, update) in updates {
            repaint = true;
            match update {
                Update::Progress(time, done, total) => {
                    self.record_progress(job, time, done, total);
                }
                Update::Frame(time, frame) => {
                    if let (Job::Detect(set), Some(scene)) = (job, self.scene.as_mut()) {
                        scene.install(set, time, frame);
                    }
                }
                Update::Tracks(tracks) => {
                    if let (Job::Track(set), Some(scene)) = (job, self.scene.as_mut()) {
                        scene.install_tracks(set, tracks);
                    }
                }
                Update::Relation(time, frame) => {
                    if let (Job::Relation { source, target }, Some(scene)) =
                        (job, self.scene.as_mut())
                    {
                        scene.install_relation(source, target, time, frame);
                    }
                }
                Update::Coloc(series, region) => {
                    let pair = series.pair();
                    if let Some(scene) = self.scene.as_mut()
                        && scene.coloc.pair() == Some(pair)
                        && scene.region == region
                        && series.mask() == scene.mask.map(|set| set.raw())
                    {
                        scene.coloc.install(*series);
                        if let Some((current, thresholds)) = self.coloc_thresholds
                            && current == pair
                        {
                            scene.coloc.set_thresholds(thresholds);
                        }
                    }
                }
                Update::Done => {
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    finished.push(job);
                    if let Job::Detect(set) = job {
                        detected.push(set);
                    }
                }
                Update::Failed(detail) => {
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    if self
                        .recipe
                        .as_ref()
                        .is_some_and(|recipe| recipe.job() == Some(job))
                    {
                        self.abort_recipe();
                    }
                    self.message = Some(Message::error(detail));
                }
            }
        }
        removed.sort_unstable();
        removed.dedup();
        for index in removed.into_iter().rev() {
            self.analysis.remove(index);
        }
        for job in finished {
            self.advance_recipe(job);
        }
        for set in detected {
            self.start_tracks(set);
            self.resume_relations(set);
        }
        repaint
    }

    fn record_progress(&mut self, job: Job, time: u32, done: usize, total: usize) {
        let progress = Progress {
            job,
            time,
            done,
            total,
        };
        if let Some(existing) = self
            .progress
            .iter_mut()
            .find(|progress| progress.job == job)
        {
            *existing = progress;
        } else {
            self.progress.push(progress);
        }
    }

    fn cancel(&mut self, job: Job) {
        if self
            .recipe
            .as_ref()
            .is_some_and(|recipe| recipe.job() == Some(job))
        {
            self.abort_recipe();
        }
        self.analysis.retain(|analysis| analysis.job() != job);
        self.progress.retain(|progress| progress.job != job);
    }

    fn defer_relations(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let mut pairs = Vec::new();
        for object in scene.objects.iter() {
            for relation in object.relations() {
                let pair = (object.id(), relation.target(), relation.shift());
                if (pair.0 == set || pair.1 == set) && !self.resume.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        self.resume.extend(pairs);
    }

    fn resume_relations(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let busy = |pair: &(ObjectSetId, ObjectSetId, i32)| {
            self.analysis.iter().any(|analysis| {
                analysis.job() == Job::Detect(pair.0) || analysis.job() == Job::Detect(pair.1)
            })
        };
        let ready = self
            .resume
            .iter()
            .copied()
            .filter(|pair| (pair.0 == set || pair.1 == set) && !busy(pair))
            .filter(|pair| {
                scene
                    .objects
                    .get(pair.0)
                    .is_some_and(|object| object.detected() > 0)
                    && scene
                        .objects
                        .get(pair.1)
                        .is_some_and(|object| object.detected() > 0)
            })
            .collect::<Vec<_>>();
        self.resume
            .retain(|candidate| !(candidate.0 == set || candidate.1 == set));
        for (source, target, shift) in ready {
            self.add_relation(source, target, shift);
        }
    }

    fn clear_relations(&mut self, set: ObjectSetId) {
        self.analysis.retain(|analysis| {
            !matches!(analysis.job(), Job::Relation { .. } if analysis.job().involves(set))
        });
        self.progress.retain(
            |progress| !matches!(progress.job, Job::Relation { .. } if progress.job.involves(set)),
        );
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_relations(set);
        }
    }

    fn add_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
        let busy = self.analysis.iter().any(
            |analysis| matches!(analysis.job(), Job::Detect(id) if id == source || id == target),
        );
        if busy {
            self.message = Some(Message::error(
                "the object sets are being detected before they relate",
            ));
            return;
        }
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if scene
            .objects
            .get(source)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the object set holds no objects to relate from",
            ));
            return;
        }
        if scene
            .objects
            .get(target)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the target object set holds no objects to relate to",
            ));
            return;
        }
        if !scene.add_relation(source, target, shift) {
            return;
        }
        self.start_relation(source, target);
    }

    fn set_relation_shift(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if !scene.set_relation_shift(source, target, shift) {
            return;
        }
        self.cancel(Job::Relation { source, target });
        self.start_relation(source, target);
    }

    fn remove_relation(&mut self, source: ObjectSetId, target: ObjectSetId) {
        self.cancel(Job::Relation { source, target });
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_relation(source, target);
        }
    }

    fn start_relation(&mut self, source: ObjectSetId, target: ObjectSetId) {
        let job = Job::Relation { source, target };
        if self.loading.is_some() || self.analysis.iter().any(|analysis| analysis.job() == job) {
            return;
        }
        let prepared = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            let (Some(from), Some(to)) = (scene.objects.get(source), scene.objects.get(target))
            else {
                return;
            };
            if to.detected() == 0 {
                self.message = Some(Message::error(
                    "the target object set holds no objects to relate to",
                ));
                return;
            }
            let shift = from.relation(target).map_or(0, |relation| relation.shift());
            let frames = scene.frame_count();
            let source_frames = (0..frames)
                .map(|time| from.frame_shared(time))
                .collect::<Vec<_>>();
            let target_frames = (0..frames)
                .map(|time| to.frame_shared(time))
                .collect::<Vec<_>>();
            (source_frames, target_frames, shift, frames as usize)
        };
        let (source_frames, target_frames, shift, total) = prepared;
        self.message = None;
        self.record_progress(job, 0, 0, total);
        self.analysis
            .push(crate::relation(job, source_frames, target_frames, shift, 0));
    }

    fn start_detect(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Detect(set))
        {
            return;
        }
        self.cancel(Job::Track(set));
        if self.recipe.is_none() {
            self.defer_relations(set);
            self.clear_relations(set);
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_tracks(set);
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let channel = object.channel();
        let params = object.params();
        let region = scene.region;
        let seeds = scene.seed_frames(set).unwrap_or_default();
        let roi = match scene.mask {
            None => None,
            Some(source) => {
                if source == set {
                    self.message = Some(Message::error(
                        "an object set cannot be limited to its own objects",
                    ));
                    return;
                }
                let detected = scene
                    .objects
                    .get(source)
                    .is_some_and(|set| set.detected() > 0);
                match scene.mask_frames() {
                    Some(frames) if detected => Some(frames),
                    _ => {
                        self.message = Some(Message::error(
                            "the analysis mask holds no detected objects",
                        ));
                        return;
                    }
                }
            }
        };
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return;
        }
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        self.message = None;
        self.record_progress(Job::Detect(set), 0, 0, 0);
        self.analysis.push(crate::detect(
            Job::Detect(set),
            source,
            DetectRequest::new(channel, params, region, roi, seeds, 0, self.store()),
        ));
    }

    fn store(&self) -> Arc<MeshStore> {
        Arc::clone(&self.store)
    }

    fn start_tracks(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job().installs(set))
        {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() < 2 {
            return;
        }
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if !object.kind().trackable() {
            return;
        }
        if object.detected() == 0 {
            return;
        }
        let mut detections = vec![Vec::new(); scene.frame_count() as usize];
        for (time, frame) in object.frames() {
            detections[time as usize] = frame.centroids();
        }
        let params = object
            .track_style()
            .expect("a trackable object carries track parameters")
            .params();
        let frame_interval = scene.dataset.frame_interval();
        self.message = None;
        self.record_progress(Job::Track(set), 0, 0, detections.len());
        self.analysis.push(crate::track(
            Job::Track(set),
            detections,
            params,
            frame_interval,
        ));
    }

    fn start_coloc(&mut self) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Coloc)
        {
            return;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(pair) = scene.coloc.pair() else {
            self.message = Some(Message::error(
                "the dataset needs two channels to colocalize",
            ));
            return;
        };
        if [pair.a(), pair.b()]
            .iter()
            .any(|channel| scene.dataset.channels()[*channel].intensity().span() <= 0.0)
        {
            self.message = Some(Message::error(
                "the selected channels have no intensity range to colocalize",
            ));
            return;
        }
        let region = scene.region;
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        let settings = scene.coloc.settings();
        let mask = scene.mask;
        let roi = match mask {
            None => None,
            Some(mask) => {
                let detected = scene
                    .objects
                    .get(mask)
                    .is_some_and(|set| set.detected() > 0);
                match scene.mask_frames() {
                    Some(frames) if detected => Some(frames),
                    _ => {
                        self.message = Some(Message::error(
                            "the analysis mask holds no detected objects",
                        ));
                        return;
                    }
                }
            }
        };
        self.message = None;
        self.record_progress(Job::Coloc, 0, 0, 0);
        self.analysis.push(crate::coloc(
            Job::Coloc,
            source,
            crate::ColocRequest::new(pair, region, roi, mask.map(|set| set.raw()), settings, 0),
        ));
    }

    fn export_coloc(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(result) = scene.coloc.result() else {
            return;
        };
        let pair = result.pair();
        let csv = result.series().csv(result.thresholds());
        let file_name = format!(
            "{}-{}-{}-colocalization.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(scene.dataset.channels()[pair.a()].name()),
            crate::slug(scene.dataset.channels()[pair.b()].name())
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export colocalization statistics")
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export colocalization to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn export_measure(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = self.measure.set() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(csv) = self.measure.csv() else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let group = match self.measure.group() {
            Group::Object => "objects",
            Group::Track => "tracks",
            Group::Timepoint => "timepoints",
        };
        let file_name = format!(
            "{}-{}-{}-{group}.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(channel),
            crate::slug(object.name())
        );
        let title = format!("Export {} statistics", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export statistics to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn capture_protocol(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let time = scene.time();
        let view = self
            .current_view
            .as_ref()
            .map(|view| ViewSpec::of(view, time));
        match Protocol::capture(scene, &self.measure, view) {
            Ok(protocol) => self.set_protocol(protocol),
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn apply_protocol(&mut self) {
        let Some(protocol) = self.protocol.clone() else {
            self.message = Some(Message::error("capture or load a protocol first"));
            return;
        };
        if self.loading.is_some() {
            return;
        }
        if self.source.is_none() || self.scene.is_none() {
            self.message = Some(Message::error("open a dataset before applying a protocol"));
            return;
        }
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.resume.clear();
        if let Some(scene) = self.scene.as_ref()
            && let Err(error) = protocol.validate(scene)
        {
            self.message = Some(Message::error(error.to_string()));
            return;
        }
        let staged = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            scene.clear_objects();
            match protocol.stage(scene) {
                Ok(staged) => staged,
                Err(error) => {
                    self.message = Some(Message::error(error.to_string()));
                    return;
                }
            }
        };
        let sets = staged.sets().to_vec();
        let pairs = match protocol.coloc() {
            Some(coloc) => staged
                .pairs()
                .iter()
                .zip(&coloc.pairs)
                .map(|(pair, spec)| {
                    (
                        *pair,
                        ColocThresholds::new(spec.thresholds[0], spec.thresholds[1]),
                    )
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let settings = protocol
            .coloc()
            .map_or_else(Default::default, |coloc| coloc.settings());
        self.recipe = Some(Recipe::new(protocol.steps(), sets, pairs, settings));
        self.message = None;
        self.drive_recipe();
    }

    fn drive_recipe(&mut self) {
        let Some(recipe) = self.recipe.as_ref() else {
            return;
        };
        let settings = recipe.settings();
        let next = match recipe.step() {
            Step::Detect { set, mask } => Next::Detect {
                set: recipe.set(set),
                mask: recipe.mask(mask),
            },
            Step::Relation { source, target } => Next::Relation {
                source: recipe.set(source),
                target: recipe.set(target),
            },
            Step::Coloc { pair, mask } => {
                let (pair, thresholds) = recipe.pair(pair);
                Next::Coloc {
                    pair,
                    thresholds,
                    mask: recipe.mask(mask),
                }
            }
            Step::Present => Next::Present,
        };
        match next {
            Next::Detect { set, mask } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask(mask);
                }
                self.start_detect(set);
            }
            Next::Relation { source, target } => self.start_relation(source, target),
            Next::Coloc {
                pair,
                thresholds,
                mask,
            } => {
                self.coloc_thresholds = Some((pair, thresholds));
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.set_settings(settings);
                    scene.coloc.set_pair(pair);
                    scene.set_mask(mask);
                }
                self.start_coloc();
            }
            Next::Present => {
                self.present_recipe();
                return;
            }
        }
        let running = self.recipe.as_ref().is_some_and(|recipe| {
            self.analysis
                .iter()
                .any(|analysis| Some(analysis.job()) == recipe.job())
        });
        if !running && self.recipe.is_some() {
            self.abort_recipe();
            if self.message.is_none() {
                self.message = Some(Message::error("the protocol could not start its next step"));
            }
        }
    }

    fn advance_recipe(&mut self, job: Job) {
        let Some(recipe) = self.recipe.as_mut() else {
            return;
        };
        if recipe.job() != Some(job) {
            return;
        }
        recipe.advance();
        self.drive_recipe();
    }

    fn present_recipe(&mut self) {
        let Some(recipe) = self.recipe.take() else {
            return;
        };
        self.coloc_thresholds = None;
        let Some(protocol) = self.protocol.clone() else {
            return;
        };
        if let Some(view) = protocol.view() {
            if let Some(scene) = self.scene.as_mut() {
                let time = view.time.min(scene.frame_count().saturating_sub(1));
                scene.set_time(time);
                self.in_flight.retain(|request| request.time >= time);
                if let Some(source) = self.source.as_ref() {
                    source.set_time(time);
                }
            }
            if let Some(timeline) = view.timeline() {
                self.timeline = timeline;
            }
            if let Some(export) = view.export() {
                self.export = export;
            }
            self.view = Some(ViewState {
                camera: view.camera(),
                section: view.section(),
                split: view.split,
                timeline: self.timeline.clone(),
                export: self.export,
            });
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        protocol.present_measure(recipe.sets(), &mut self.measure, scene);
    }

    fn abort_recipe(&mut self) {
        self.recipe = None;
        self.coloc_thresholds = None;
    }

    fn load_protocol(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open an analysis protocol")
            .add_filter("grana protocol", &["json"])
            .pick_file()
        else {
            return;
        };
        match Protocol::read(&path) {
            Ok(protocol) => self.set_protocol(protocol),
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn save_protocol(&mut self) {
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let Some(protocol) = self.protocol.as_ref() else {
            return;
        };
        let suggested = format!(
            "{}-protocol.json",
            crate::slug(
                self.scene
                    .as_ref()
                    .map_or("grana", |scene| scene.dataset.name())
            )
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save the analysis protocol")
            .set_file_name(suggested)
            .add_filter("grana protocol", &["json"])
            .save_file()
        else {
            return;
        };
        match protocol.write(&path) {
            Ok(()) => self.batch_message = None,
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn add_batch_datasets(&mut self) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("Choose a folder of datasets")
            .pick_folder()
        else {
            return;
        };
        match crate::discover(&root) {
            Ok(found) => {
                let mut added = 0;
                for path in found {
                    if !self.batch_datasets.contains(&path) {
                        self.batch_datasets.push(path);
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

    fn choose_batch_output(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a folder for the batch statistics")
            .pick_folder()
        else {
            return;
        };
        self.batch_output = Some(path);
        self.batch_message = None;
    }

    fn set_batch_jobs(&mut self, jobs: u32) {
        if self.batch_running {
            return;
        }
        self.batch_jobs = jobs.clamp(1, crate::MAX_JOBS as u32) as usize;
    }

    fn run_batch(&mut self) {
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
        }));
    }

    fn reset_batch_rows(&mut self) {
        self.batch_rows = self
            .batch_datasets
            .iter()
            .map(|path| BatchDataset {
                name: crate::dataset_name(path),
                state: "queued".to_string(),
                detail: String::new(),
            })
            .collect();
    }

    fn poll_batch(&mut self) -> bool {
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

    fn remove_object(&mut self, set: ObjectSetId) {
        self.cancel(Job::Detect(set));
        self.cancel(Job::Track(set));
        self.clear_relations(set);
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_object(set);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
