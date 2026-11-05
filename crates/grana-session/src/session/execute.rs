use super::*;

pub(super) struct Recording {
    edit: Edit,
    snapshot: Snapshot,
    hash: u64,
    analyses: usize,
}

impl Session {
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
            Command::ClearCache => self.clear_cache(),
            Command::SetTime(time) => self.set_time(time),
            Command::SelectSeries(index) => {
                if index != self.series_index && !self.select_series(index) {
                    self.message = Some(Message::error("the dataset does not hold that series"));
                }
            }
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
                match removed {
                    Some(Ok(true)) => {
                        self.defer_relations(set);
                        self.refresh_dependents(set);
                        self.resume_relations(set);
                    }
                    Some(Err(detail)) => self.message = Some(Message::error(detail)),
                    Some(Ok(false)) | None => {}
                }
            }
            Command::DeleteFiltered => {
                self.delete_filtered();
            }
            Command::ExtractFiltered => {
                self.extract_filtered();
            }
            Command::DuplicateObject(set) => {
                let copied = self
                    .scene
                    .as_mut()
                    .and_then(|scene| scene.duplicate_object(set));
                match copied {
                    Some(copied) => {
                        self.defer_relations(copied);
                        self.resume_relations(copied);
                        self.message = None;
                    }
                    None => {
                        self.message = Some(Message::error("the object set cannot be copied"));
                    }
                }
            }
            Command::MergeObjects { source, target } => {
                self.merge_objects(source, target);
            }
            Command::RemoveObject(set) => self.remove_object(set),
            Command::Detect(set) => self.detect_root(set),
            Command::CancelDetect(set) => {
                let set = self.detection_root(set).unwrap_or(set);
                self.cancel(Job::Detect(set));
                self.cancel(Job::Recut(set));
                if self.recut.is_some_and(|(pending, _)| pending == set) {
                    self.recut = None;
                }
            }
            Command::ClearObject(set) => {
                self.cancel(Job::Detect(set));
                self.cancel(Job::Recut(set));
                self.cancel(Job::Track(set));
                if self.recut.is_some_and(|(pending, _)| pending == set) {
                    self.recut = None;
                }
                self.clear_relations(set);
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_object(set);
                }
                self.refresh_dependents(set);
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
            Command::SetSeedSource { set, source } => {
                self.set_seed_source(set, source);
            }
            Command::AddRelation {
                source,
                target,
                shift,
            } => self.add_relation(source, target, shift),
            Command::MoveRelation {
                source,
                target,
                shift,
                next,
            } => self.move_relation(source, target, shift, next),
            Command::RemoveRelation {
                source,
                target,
                shift,
            } => self.remove_relation(source, target, shift),
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
            Command::SetObjectLabels { set, labels } => {
                if let Some(target) = self.object_mut(set) {
                    target.set_labels(labels);
                }
            }
            Command::SetLabelMetric { set, metric } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_label_metric(set, metric);
                }
            }
            Command::SetObjectHidden {
                set,
                time,
                objects,
                hidden,
            } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_object_hidden(set, time, &objects, hidden);
                }
            }
            Command::ClearHidden(set) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_hidden(set);
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
                let root = self.detection_root(set).unwrap_or(set);
                let previous = self.object_mut(root).map(|target| {
                    let previous = target.params();
                    target.set_params(params);
                    previous
                });
                if let Some(previous) = previous {
                    self.recut(root, previous, params);
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
            Command::SetClip(clip) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_clip(clip);
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
            Command::ComputeDrift => self.start_drift(),
            Command::CancelDrift => self.cancel(Job::Drift),
            Command::ClearDrift => self.clear_drift(),
            Command::SetDriftReference(reference) => self.set_drift_reference(reference),
            Command::SetDriftOffset { time, offset } => self.set_drift_offset(time, offset),
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
            Command::PauseBatch => self.set_batch_paused(true),
            Command::ResumeBatch => self.set_batch_paused(false),
            Command::CancelBatch => {
                if let Some(batch) = &self.batch {
                    batch.control().cancel();
                }
                self.batch_paused = false;
            }
            Command::SetKeyframe => effects.keyframe = true,
            Command::ClearKeyframes => self.timeline.clear(),
            Command::DeleteKeyframe(index) => {
                self.timeline.remove(index);
            }
            Command::SetTimelineFps(fps) => self.timeline.set_fps(fps),
            Command::SetTimelineDuration(duration) => self.timeline.set_duration(duration),
            Command::ExportMovie { width } => effects.export_movie = Some(width),
            Command::ExportAnimation { width } => effects.export_animation = Some(width),
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

    pub(super) fn begin_edit(&self, command: &Command) -> Option<Recording> {
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

    pub(super) fn commit_edit(&mut self, recording: Recording) {
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

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        let scene = self.scene.as_ref()?;
        Some(Snapshot::capture(scene, &self.measure, &self.timeline))
    }

    pub(super) fn undo(&mut self) -> Effects {
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

    pub(super) fn redo(&mut self) -> Effects {
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

    pub(super) fn abandon(&mut self, edit: Edit) {
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

    pub(super) fn restore(&mut self, snapshot: Snapshot) {
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
        self.realign();
        self.message = None;
    }

    pub(super) fn object_mut(&mut self, set: ObjectSetId) -> Option<&mut grana_scene::ObjectSet> {
        self.scene.as_mut()?.objects.get_mut(set)
    }
}
