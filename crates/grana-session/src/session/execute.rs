use super::*;

use grana_scene::{SegmentModel, SegmentParams};

use super::edit::{MaskPaint, SurfaceEditRequest};

pub(super) struct Recording {
    edit: Edit,
    snapshot: Snapshot,
    hash: u64,
    analyses: usize,
}

impl Session {
    pub fn execute(&mut self, command: Command) -> Effects {
        if !self.access().permits(&command) {
            self.message = Some(Message::info(
                "this viewer shows a recorded analysis and cannot change it",
            ));
            return Effects {
                repaint: true,
                ..Effects::default()
            };
        }
        self.settle_measure();
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
            Command::OpenTiles => effects.open_tiles = true,
            Command::OpenPath(path) => self.open_path(path),
            Command::OpenRemote(url) => self.begin_remote(url),
            Command::OpenRecent(index) => self.open_recent(index),
            Command::RecoverAnalysis => self.recover(),
            Command::DiscardAnalysis => {
                self.discard_recovery();
            }
            Command::OpenProject => effects.open_project = true,
            Command::OpenPackage => effects.open_package = true,
            Command::SaveProject => effects.save_project = Some(false),
            Command::SaveProjectAs => effects.save_project = Some(true),
            Command::SaveSnapshot => effects.snapshot = true,
            Command::SaveFigure => effects.figure = true,
            Command::SaveLineage => effects.lineage = true,
            Command::SaveChart => effects.chart = true,
            Command::SaveComparison => effects.comparison = true,
            Command::SetTime(time) => self.set_time(time),
            Command::SelectSeries(index) => {
                if index != self.series_index && !self.select_series(index) {
                    self.message = Some(Message::error("the dataset does not hold that series"));
                }
            }
            Command::SelectField(index) => {
                self.select_field(index);
            }
            Command::Select(selection) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.select(selection);
                }
            }
            Command::ToggleSelect(selection) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.toggle_select(selection);
                }
            }
            Command::AddObject(kind) => {
                if let Some(scene) = self.scene.as_mut() {
                    let channel = scene.selected;
                    scene.add_object(kind, channel);
                }
            }
            Command::AddSegment => {
                let Some(model) = SegmentModel::preferred() else {
                    self.message = Some(Message::error(
                        "no segmentation model is licensed for this build",
                    ));
                    return Effects::default();
                };
                if let Some(scene) = self.scene.as_mut() {
                    let channel = scene.selected;
                    scene.add_segment(channel, SegmentParams::default().with_model(model));
                }
            }
            Command::SetSegment { set, params } => {
                let changed = self
                    .scene
                    .as_mut()
                    .and_then(|scene| scene.objects.get_mut(set))
                    .is_some_and(|object| object.set_segment_params(params));
                if changed {
                    self.message = Some(Message::info(
                        "detect the object set to apply the new segmentation settings",
                    ));
                }
            }
            Command::AddMaskChannel { set } => self.add_mask_channel(set),
            Command::AddRestoreChannel {
                source,
                psf,
                iterations,
            } => self.add_restore_channel(source, psf, iterations),
            Command::AddSmoothChannel { source, sigma } => {
                self.add_smooth_channel(source, sigma);
            }
            Command::AddBackgroundChannel { source, radius } => {
                self.add_background_channel(source, radius);
            }
            Command::AddRatioChannel {
                numerator,
                denominator,
            } => self.add_ratio_channel(numerator, denominator),
            Command::AddBleachChannel { source } => self.add_bleach_channel(source),
            Command::SetSmoothChannel { channel, sigma } => {
                self.set_smooth_channel(channel, sigma);
            }
            Command::SetBackgroundChannel { channel, radius } => {
                self.set_background_channel(channel, radius);
            }
            Command::SetRatioChannel {
                channel,
                denominator,
            } => self.set_ratio_channel(channel, denominator),
            Command::CalibratePsf {
                source,
                threshold,
                separation,
            } => self.begin_calibration(source, threshold, separation),
            Command::SetRestore {
                channel,
                psf,
                iterations,
            } => self.set_restore_channel(channel, psf, iterations),
            Command::RemoveDerivedChannel { channel } => self.remove_derived_channel(channel),
            Command::ImportLabels { label } => self.import_labels(&label),
            Command::ImportTracks => self.choose_import_tracks(),
            Command::ImportLocalizations => self.choose_import_localizations(),
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
            Command::MergeSurfaces { set, time, objects } => {
                self.edit_surfaces(set, time, SurfaceEditRequest::Merge(objects));
            }
            Command::SplitSurface { set, time, object } => {
                self.edit_surfaces(set, time, SurfaceEditRequest::Split(object));
            }
            Command::PaintMask {
                set,
                frame,
                anchor,
                tool,
                mode,
                radius,
                tolerance,
                points,
            } => {
                self.paint_mask(MaskPaint {
                    set,
                    frame,
                    anchor,
                    tool,
                    mode,
                    radius,
                    tolerance,
                    points,
                });
            }
            Command::ClearMask(set) => {
                self.clear_mask(set);
            }
            Command::RemoveObject(set) => {
                let masked = self.scene.as_ref().is_some_and(|scene| {
                    scene
                        .derived()
                        .iter()
                        .any(|entry| entry.spec().mask() == Some(set))
                });
                if masked {
                    self.message = Some(Message::error(
                        "a mask channel derives from this object set",
                    ));
                } else {
                    self.remove_object(set);
                }
            }
            Command::Detect(set) => self.detect_root(set),
            Command::CancelDetect(set) => {
                let set = self.detection_root(set).unwrap_or(set);
                if self
                    .analysis
                    .iter()
                    .any(|analysis| analysis.job() == Job::Import(set))
                {
                    self.remove_object(set);
                    self.message = None;
                } else {
                    self.cancel(Job::Detect(set));
                    self.cancel(Job::Recut(set));
                    if self.recut.is_some_and(|(pending, _)| pending == set) {
                        self.recut = None;
                    }
                }
            }
            Command::ClearObject(set) => {
                self.cancel(Job::Detect(set));
                self.cancel(Job::Import(set));
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
            Command::SetVolumeMode { channel, mode } => {
                if let Some(scene) = self.scene.as_mut()
                    && channel < scene.channels.len()
                {
                    scene.set_volume_mode(channel, mode);
                }
            }
            Command::SetIsovalue { channel, isovalue } => {
                if let Some(scene) = self.scene.as_mut()
                    && channel < scene.channels.len()
                {
                    scene.set_isovalue(channel, isovalue.clamp(0.0, 1.0));
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
            Command::SetObjectMaterial { set, material } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_object_material(set, material);
                }
            }
            Command::SetObjectParams { set, params } => {
                let root = self.detection_root(set).unwrap_or(set);
                let imported = self
                    .scene
                    .as_ref()
                    .and_then(|scene| scene.objects.get(root))
                    .is_some_and(|object| object.provenance().is_imported());
                if imported {
                    return effects;
                }
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
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_object_coloring(set, coloring);
                }
            }
            Command::SetMask(source) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask(source);
                }
            }
            Command::SetMaskRadius(radius) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask_radius(radius);
                }
            }
            Command::SetMaskFilter(filter) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask_filter(filter);
                }
            }
            Command::AddPoint { at } => {
                if let Some(scene) = self.scene.as_mut() {
                    let frame = Some(scene.time());
                    let index = scene.annotations.add_point(at, frame);
                    scene.annotations.select(Some(index));
                }
            }
            Command::AddDistance { from, to } => {
                if let Some(scene) = self.scene.as_mut() {
                    let frame = Some(scene.time());
                    let index = scene.annotations.add_distance(from, to, frame);
                    scene.annotations.select(Some(index));
                }
            }
            Command::SelectAnnotation(index) => {
                let frame = self.scene.as_mut().and_then(|scene| {
                    scene.annotations.select(index);
                    let index = index?;
                    scene.annotations.get(index)?.frame()
                });
                if let Some(frame) = frame {
                    self.set_time(frame);
                }
            }
            Command::SetAnnotationVisible { index, visible } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.set_visible(index, visible);
                }
            }
            Command::SetAnnotationKind { index, kind } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.set_kind(index, kind);
                }
            }
            Command::SetAnnotationFrame { index, frame } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.set_frame(index, frame);
                }
            }
            Command::SampleProfile { index } => self.sample_profile(index),
            Command::RemoveAnnotation(index) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.remove(index);
                }
            }
            Command::ClearAnnotations => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.clear();
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
            Command::DivideTracks {
                set,
                parent,
                first,
                second,
            } => {
                let divided = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.divide_tracks(set, parent, first, second));
                self.message = (!divided).then(|| {
                    Message::error("a division links a track end to two later lineage starts")
                });
            }
            Command::MergeTracks {
                set,
                first,
                second,
                child,
            } => {
                let merged = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.merge_tracks(set, first, second, child));
                self.message = (!merged).then(|| {
                    Message::error("a merge joins two earlier track ends to a later lineage start")
                });
            }
            Command::ReparentTrack { set, child, parent } => {
                let adopted = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.reparent_track(set, child, parent));
                self.message = (!adopted)
                    .then(|| Message::error("a parent track ends before its daughter starts"));
            }
            Command::DetachTrack { set, child } => {
                let detached = self
                    .scene
                    .as_mut()
                    .is_some_and(|scene| scene.detach_track(set, child));
                self.message = (!detached)
                    .then(|| Message::error("the track starts a lineage and has no parent"));
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
            Command::TrainClassifier { set, features } => self.train_classifier(set, features),
            Command::ClearClassifier(set) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_classifier(set);
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
                    scene.coloc.set_pair(pair, scene.channels.len());
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
            Command::ComputeObjectColoc(set) => self.start_object_coloc(set),
            Command::CancelObjectColoc(set) => self.cancel(Job::ObjectColoc(set)),
            Command::ClearObjectColoc(set) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_object_coloc(set);
                }
            }
            Command::AnalyzeLocalizations(set) => self.start_localizations(set),
            Command::SetLocalizationSpec { set, spec } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_localization_spec(set, spec);
                }
            }
            Command::SetLocalizationProjected { set, projected } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_localization_projected(set, projected);
                }
            }
            Command::SetLocalizationClusters { set, clusters } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_localization_clusters(set, clusters);
                }
            }
            Command::ClusterLocalizations(set) => self.start_clusters(set),
            Command::EstimateLocalizationClusters(set) => self.estimate_clusters(set),
            Command::CancelClusters(set) => self.cancel(Job::Clusters(set)),
            Command::CancelLocalizations(set) => self.cancel_localizations(set),
            Command::ClearLocalizations(set) => self.clear_localizations(set),
            Command::ExportLocalizationImage(set) => self.export_localization_image(set),
            Command::ComputeDrift => self.start_drift(),
            Command::ComputeLocalizationDrift(set) => self.start_localization_drift(set),
            Command::CancelDrift => self.cancel(Job::Drift),
            Command::ClearDrift => self.clear_drift(),
            Command::SetDriftReference(reference) => self.set_drift_reference(reference),
            Command::SetDriftOffset { time, offset } => self.set_drift_offset(time, offset),
            Command::RegisterChannels(reference) => self.start_registration(reference),
            Command::CancelRegistration => self.cancel(Job::Register),
            Command::ClearRegistration => self.clear_registration(),
            Command::ExportColoc => self.export_coloc(),
            Command::ExportMeasure => self.export_measure(),
            Command::ExportProvenance => self.export_provenance(),
            Command::ExportPackage => self.export_package(),
            Command::ExportRelation {
                source,
                target,
                shift,
            } => self.export_relation(source, target, shift),
            Command::ExportMesh { set, format } => self.export_mesh(set, format),
            Command::ExportLabels(set) => self.export_labels(set),
            Command::ExportAllLabels => self.export_all_labels(),
            Command::ExportDataset => self.export_dataset(),
            Command::ExportPlate => self.export_plate(),
            Command::ExportGeff(set) => self.choose_export_geff(set),
            Command::ExportLocalizations(set) => self.choose_export_localizations(set),
            Command::CaptureProtocol => self.capture_protocol(),
            Command::ApplyProtocol => self.apply_protocol(),
            Command::LoadProtocol => self.load_protocol(),
            Command::SaveProtocol => self.save_protocol(),
            Command::AddBatchDatasets => self.add_batch_datasets(),
            Command::ClearBatchDatasets => {
                self.batch_datasets.clear();
                self.batch_rows.clear();
                self.batch_review = None;
                self.batch_review_pending = None;
            }
            Command::ChooseBatchOutput => self.choose_batch_output(),
            Command::OpenBatchRun => self.open_batch_run(),
            Command::ReviewBatchDataset(index) => self.review_batch_dataset(index),
            Command::RecordBatchCorrection => self.record_batch_correction(),
            Command::ClearBatchCorrection => self.clear_batch_correction(),
            Command::SetBatchJobs(jobs) => self.set_batch_jobs(jobs),
            Command::SetBatchProject(project) => {
                self.batch_project = project;
                self.reset_batch_rows();
            }
            Command::SetBatchChannels(channels) => {
                self.batch_channels = channels;
                self.reset_batch_rows();
            }
            Command::SetStitchGrid { rows, columns } => {
                self.set_stitch_grid(rows, columns);
            }
            Command::SetStitchChannel(channel) => {
                self.set_stitch_channel(channel);
            }
            Command::SetStitchSeries(series) => {
                self.set_stitch_series(series);
            }
            Command::ConfirmStitch => {
                self.confirm_stitch();
            }
            Command::CancelStitch => {
                self.cancel_stitch();
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
            Command::SaveView => {
                self.save_view();
            }
            Command::ApplyView(index) => {
                self.apply_view(index);
            }
            Command::RemoveView(index) => {
                self.remove_view(index);
            }
            Command::ClearKeyframes => self.timeline.clear(),
            Command::DeleteKeyframe(index) => {
                self.timeline.remove(index);
            }
            Command::SetTimelineFps(fps) => self.timeline.set_fps(fps),
            Command::SetTimelineDuration(duration) => self.timeline.set_duration(duration),
            Command::ExportMovie { transparent } => effects.movie = Some(transparent),
            Command::SetExport(export) => self.export = export,
            Command::ExportSequence => effects.sequence = true,
            Command::CancelExport => effects.cancel_export = true,
            Command::Undo | Command::Redo => {
                unreachable!("undo and redo run before the document commands")
            }
        }
        if let Some(recording) = recording {
            self.commit_edit(recording);
        }
        self.touch();
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
            self.deferred = false;
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
        self.touch();
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
        self.touch();
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
                Action::Cluster => self.cancel(Job::Clusters(set)),
                Action::ObjectColoc => self.cancel(Job::ObjectColoc(set)),
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
        let derived = scene.derived_shared();
        scene.restore(state);
        let derived = scene.derived_shared() != derived;
        self.measure.restore(measure, scene);
        self.measure.set_roi(scene.region_scope());
        self.timeline = timeline;
        if let Some(source) = &self.source {
            source.set_time(scene.time());
        }
        self.realign();
        self.message = None;
        if derived {
            self.refresh_derived_layout();
        }
    }

    pub(super) fn object_mut(&mut self, set: ObjectSetId) -> Option<&mut grana_scene::ObjectSet> {
        self.scene.as_mut()?.objects.get_mut(set)
    }
}
