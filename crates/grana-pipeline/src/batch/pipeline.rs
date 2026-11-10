use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_animation::Timeline;
use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::{Format, Image, Source};
use grana_measure::{ColocPair, ColocSeries, Group, Lasso};
use grana_mesh::Format as MeshFormat;
use grana_object::Roi;
use grana_project::{Export, NamedView, Project, Source as ProjectSource, ViewState};
use grana_scene::{
    Dataset, MaskFilter, Measure, ObjectFrame, ObjectParams, ObjectSetId, Scene, slug,
};
use grana_volume::{Pyramid, Region};

use crate::capture::{Capture, CaptureRequest, Kind as CaptureKind};
use crate::channels::Channels;
use crate::event::Event as RunEvent;
use crate::label::Labeling;
use crate::mesh::{self, Export as MeshExport};
use crate::run;
use grana_protocol::{Protocol, ProtocolError, SnapshotSpec, Step, ViewSpec};

use super::channel;
use super::profile;
use super::quality::{self, Quality};
use super::resume::{Record, build};
use super::{DatasetEntry, Event, PARTIAL, Progress, RESULT, Task};

const COLOC_PLOT: [u32; 2] = [720, 450];

pub(super) struct Report {
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
    pub(super) labels: Vec<String>,
}

pub(super) fn analyse(
    task: &Task<'_>,
    dataset: &DatasetEntry,
    output: &Path,
) -> Result<Report, String> {
    let folder = output.join(dataset.folder());
    let partial = output.join(format!(".{PARTIAL}-{}", dataset.key()));
    let previous = output.join(format!(".{PARTIAL}-{}-previous", dataset.key()));
    if !folder.exists() && previous.exists() {
        std::fs::rename(&previous, &folder).map_err(|error| {
            format!(
                "failed to restore {} over {}: {error}",
                previous.display(),
                folder.display()
            )
        })?;
    }
    clear(&partial)?;
    std::fs::create_dir_all(&partial)
        .map_err(|error| format!("failed to create {}: {error}", partial.display()))?;
    let outcome = write_dataset(task, dataset, &partial);
    let report = match outcome {
        Ok(report) => report,
        Err(detail) => {
            let _ = std::fs::remove_dir_all(&partial);
            return Err(detail);
        }
    };
    if let Some(parent) = folder.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(format!("failed to create {}: {error}", parent.display()));
    }
    install(&partial, &folder, &previous, dataset)?;
    Ok(report)
}

fn install(
    partial: &Path,
    folder: &Path,
    previous: &Path,
    dataset: &DatasetEntry,
) -> Result<(), String> {
    if !folder.exists() {
        return std::fs::rename(partial, folder).map_err(|error| {
            let _ = std::fs::remove_dir_all(partial);
            format!("failed to write {}: {error}", folder.display())
        });
    }
    clear(previous)?;
    std::fs::rename(folder, previous).map_err(|error| {
        let _ = std::fs::remove_dir_all(partial);
        format!(
            "failed to set aside {} for {}: {error}",
            folder.display(),
            dataset.title()
        )
    })?;
    match std::fs::rename(partial, folder) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(previous);
            Ok(())
        }
        Err(error) => {
            let restored = std::fs::rename(previous, folder);
            let _ = std::fs::remove_dir_all(partial);
            match restored {
                Ok(()) => Err(format!("failed to write {}: {error}", folder.display())),
                Err(detail) => Err(format!(
                    "failed to write {}: {error}; the previous results stay at {} ({detail})",
                    folder.display(),
                    previous.display()
                )),
            }
        }
    }
}

fn write_dataset(task: &Task<'_>, dataset: &DatasetEntry, folder: &Path) -> Result<Report, String> {
    let source = match grana_image::series_group(dataset.path(), dataset.scene())? {
        Some(group) => Source::file_group(dataset.path().to_path_buf(), dataset.scene(), group),
        None => Source::file_series(dataset.path().to_path_buf(), dataset.scene()),
    };
    let reader = Arc::new(Image::open(&source)?);
    let store = GeometryStore::temporary(DEFAULT_BUDGET)
        .map_err(|error| format!("the dataset cannot page its geometry: {error}"))?;
    let name = dataset.title();
    let mut artifacts = artifacts(task.protocol, &name, task.project, task.channels);
    let mut scene = Scene::new(dataset_of(reader.format(), &name, reader.pyramid()));
    let infos = task
        .protocol
        .derived_infos(reader.pyramid().channels())
        .map_err(|error| error.to_string())?;
    scene.set_derived_layout(infos)?;
    let staged = task
        .protocol
        .stage(&mut scene)
        .map_err(|error: ProtocolError| error.to_string())?;
    let derived = task
        .protocol
        .derived_specs(&scene, &staged)
        .map_err(|error| error.to_string())?;
    scene.bind_derived(derived)?;
    let channels = Channels::analysis(Arc::clone(&reader), &scene);
    let mut run = Run {
        task,
        dataset,
        channels,
        store: &store,
        scene,
        sets: staged.sets().to_vec(),
        pairs: staged.pairs().to_vec(),
        progress: Progress {
            index: task.index,
            events: task.events,
            cancel: task.cancel,
            gate: task.gate,
        },
    };
    let mut measure = Measure::new();
    for step in staged.steps() {
        if !task.gate.hold(task.cancel) {
            return Err("cancelled".to_string());
        }
        match step {
            Step::Drift {
                channel,
                reference,
                offsets,
            } => run.align(*channel, *reference, offsets)?,
            Step::Detect {
                set,
                mask,
                mask_radius,
                filter,
            } => run.detect(*set, *mask, *mask_radius, *filter)?,
            Step::Import { set } => run.import(*set)?,
            Step::Filter { set, .. } => run.refilter(*set)?,
            Step::Copy { set, source } => run.copy(*set, *source)?,
            Step::Edit { set, .. } => run.edit(*set)?,
            Step::Merge { set, part } => run.merge(*set, *part)?,
            Step::Relation {
                source,
                target,
                shift,
                ..
            } => run.relate(*source, *target, *shift)?,
            Step::Coloc {
                pair,
                mask,
                mask_radius,
                filter,
            } => run.colocalize(*pair, *mask, *mask_radius, *filter)?,
            Step::ObjectColoc { set, pair } => run.object_coloc(*set, *pair)?,
            Step::Labels { set } => run.label(*set)?,
            Step::Present => {
                task.protocol
                    .present_measure(&run.sets, &mut measure, &run.scene);
                measure.apply(&mut run.scene);
            }
        }
    }
    let Run {
        mut scene,
        sets,
        channels,
        ..
    } = run;
    scene.sync_masks();
    if task.channels {
        channel::write(task, &scene, &channels, folder)?;
    }
    let mut objects = 0usize;
    let mut detail = Vec::new();
    let mut quality = Vec::new();
    let keys = quality::keys(&scene);
    for (index, spec) in task.protocol.sets().iter().enumerate() {
        let table = scene
            .table(sets[index], spec.group(), spec.aggregate())
            .ok_or_else(|| format!("the set {} detected no frame", spec.name))?;
        let filters = spec.filters(&table)?;
        let lasso = spec
            .measure()
            .and_then(|measure| measure.lasso())
            .and_then(|lasso| {
                let x = table.column(&lasso.x)?;
                let y = table.column(&lasso.y)?;
                Lasso::new(x, y, lasso.points.clone())
            });
        let mut rows = table.matching(&filters, lasso.as_ref(), None);
        if spec.group() == Group::Object
            && let Some(region) = scene.region_scope()
        {
            rows.retain(|row| {
                table
                    .position(*row)
                    .is_none_or(|position| region.contains(glam::Vec3::from(position)))
            });
        }
        if let Some(measure) = spec.measure()
            && let Some(sort) = &measure.sort
        {
            let column = table.column(&sort.column).ok_or_else(|| {
                format!(
                    "the statistics of {} sort by the missing column {}",
                    spec.name, sort.column
                )
            })?;
            table.sort(&mut rows, column, sort.descending);
        }
        let file = format!("{}-{}.csv", spec.key(), spec.group().key());
        write(&folder.join(&file), &table.csv(&rows))?;
        objects += rows.len();
        let curation = scene
            .objects
            .get(sets[index])
            .map(|set| set.curation().clone())
            .unwrap_or_default();
        let removed = curation.removal_count();
        let pending = curation.pending();
        let dropped = curation.dropped();
        let mut line = format!("{}={}", spec.name, rows.len());
        if removed > 0 {
            line.push_str(&format!(" ({} removed by hand)", removed));
        }
        if pending > 0 {
            line.push_str(&format!(" ({pending} awaiting a matching object)"));
        }
        if dropped > 0 {
            line.push_str(&format!(" ({} manual edits dropped)", dropped));
        }
        detail.push(line);
        quality.push(quality::of(
            &scene,
            sets[index],
            &keys,
            &table,
            &rows,
            &name,
            dataset.group(),
        ));
    }
    for set in scene.objects.iter() {
        for relation in set.relations() {
            let Some(target) = scene.objects.get(relation.target()) else {
                continue;
            };
            if relation.computed() == 0 {
                continue;
            }
            let file = grana_measure::PairExport::file_name(
                set.key().as_str(),
                target.key().as_str(),
                relation.shift(),
            );
            let mut export = grana_measure::PairExport::new(
                scene.dataset.frame_interval(),
                set.key().as_str(),
                target.key().as_str(),
            );
            for (time, frame) in relation.frames() {
                export.push(time, frame);
            }
            write(&folder.join(&file), &export.text())?;
        }
    }
    if scene
        .annotations
        .iter()
        .any(|annotation| annotation.profile().is_some())
    {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("exporting the intensity profiles of {}", reader.name()),
        });
        profile::write(&scene, folder)?;
    }
    if task.protocol.coloc().is_some() {
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let tints = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.tint())
            .collect::<Vec<_>>();
        let mut fonts = grana_text::Fonts::system();
        for result in scene.coloc.results() {
            let pair = result.pair();
            let file = format!(
                "coloc-{}-{}.csv",
                slug(&channels[pair.a()]),
                slug(&channels[pair.b()])
            );
            write(
                &folder.join(&file),
                &result.series().csv(result.thresholds()),
            )?;
            let plot = format!(
                "coloc-{}-{}.png",
                slug(&channels[pair.a()]),
                slug(&channels[pair.b()])
            );
            let raster = colocalization(
                &mut fonts,
                result,
                &channels[pair.a()],
                &channels[pair.b()],
                tints[pair.a()],
                tints[pair.b()],
            );
            grana_raster::write(&folder.join(&plot), &raster)
                .map_err(|error| format!("failed to write {plot}: {error}"))?;
        }
    }
    if task
        .protocol
        .sets()
        .iter()
        .any(|spec| !spec.meshes.is_empty())
    {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("exporting the meshes of {}", reader.name()),
        });
        for (index, spec) in task.protocol.sets().iter().enumerate() {
            let stem = format!("{}-{}", slug(&name), spec.key());
            for time in 0..scene.frame_count() {
                for format in &spec.meshes {
                    let file = match format {
                        MeshFormat::Stl => format!("{stem}-t{time:04}"),
                        format => format!("{stem}-t{time:04}.{}", format.extension()),
                    };
                    let path = folder.join(&file);
                    match mesh::write(&scene, sets[index], time, *format, &path)? {
                        MeshExport::Written => artifacts.push(file),
                        MeshExport::Empty => {}
                    }
                }
            }
        }
    }
    let labels = scene
        .objects
        .iter()
        .filter_map(|set| set.label_export())
        .map(|export| export.path().to_string())
        .collect::<Vec<_>>();
    let report = Report {
        objects,
        detail: detail.join("; "),
        quality,
        labels,
    };
    if let Some(spec) = task.protocol.snapshot() {
        let capture = task.capture.ok_or_else(|| {
            "the protocol requests captures but this run carries no renderer".to_string()
        })?;
        let render = Channels::render(Arc::clone(&reader), &scene);
        let time = task.protocol.view().map_or(scene.time(), |view| view.time);
        scene.set_time(time.min(scene.frame_count() - 1));
        let stems = capture_stems(task.protocol);
        if spec.views().is_empty() {
            let _ = task.events.send(Event::Step {
                index: task.index,
                label: format!("rendering the captures of {}", reader.name()),
            });
            let view = view_state(task.protocol.view(), &scene);
            capture_view(capture, &render, &scene, &spec, view, folder, &stems[0])?;
        } else {
            for (name, stem) in spec.views().iter().zip(&stems) {
                let named = task
                    .protocol
                    .named_view(name)
                    .expect("a checked protocol names its views");
                let time = named.view().time.min(scene.frame_count() - 1);
                scene.set_time(time);
                let _ = task.events.send(Event::Step {
                    index: task.index,
                    label: format!("rendering the view {name} of {}", reader.name()),
                });
                let view = view_state(Some(named.view()), &scene);
                capture_view(capture, &render, &scene, &spec, view, folder, stem)?;
            }
            scene.set_time(time.min(scene.frame_count() - 1));
        }
    }
    if task.project {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("writing the project of {}", reader.name()),
        });
        let fingerprint = channels
            .image()
            .fingerprint()
            .map_err(|error| format!("the dataset cannot be fingerprinted: {error}"))?;
        let project = Project::capture(
            &scene,
            &measure,
            ProjectSource::File {
                name: name.clone(),
                path: dataset.path().to_path_buf(),
                series: dataset.scene(),
            },
            view_state(task.protocol.view(), &scene),
            task.protocol
                .views()
                .iter()
                .map(|named| {
                    NamedView::new(named.name.clone(), view_state(Some(named.view()), &scene))
                })
                .collect(),
            fingerprint,
        );
        let file = format!("{}.granaproj", slug(&name));
        let mut progress = |done: usize, total: usize| {
            let _ = task.events.send(Event::Progress {
                index: task.index,
                done,
                total,
            });
        };
        project
            .write(&folder.join(&file), task.cancel, &mut progress)
            .map_err(|error| format!("failed to write {file}: {error}"))?;
    }
    let record = serde_json::to_string(&Record {
        protocol: task.digest,
        build: build(),
        content: task.evidence.content,
        artifacts,
        labels: report.labels.clone(),
        objects: report.objects,
        detail: report.detail.clone(),
        quality: report.quality.clone(),
    })
    .map_err(|error| format!("failed to record the dataset: {error}"))?;
    write(&folder.join(RESULT), &record)?;
    Ok(report)
}

struct Run<'a, 'b> {
    task: &'b Task<'a>,
    dataset: &'b DatasetEntry,
    channels: Channels,
    store: &'b Arc<GeometryStore>,
    scene: Scene,
    sets: Vec<ObjectSetId>,
    pairs: Vec<ColocPair>,
    progress: Progress<'a>,
}

impl Run<'_, '_> {
    fn align(
        &mut self,
        channel: usize,
        reference: u32,
        offsets: &[[f32; 3]],
    ) -> Result<(), String> {
        let task = self.task;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("aligning the frames of {}", self.channels.name()),
        });
        self.scene.set_drift(grana_scene::Drift::new(
            channel,
            reference,
            offsets.to_vec(),
        ));
        Ok(())
    }

    fn detect(
        &mut self,
        id: ObjectSetId,
        mask: Option<ObjectSetId>,
        mask_radius: f32,
        filter: Option<MaskFilter>,
    ) -> Result<(), String> {
        let task = self.task;
        self.scene.set_mask(mask);
        self.scene.set_mask_radius(mask_radius);
        self.scene.set_mask_filter(filter);
        let roi = match mask {
            None => None,
            Some(mask) => {
                let detected = self
                    .scene
                    .objects
                    .get(mask)
                    .is_some_and(|set| set.detected() > 0);
                if !detected {
                    let name = self
                        .scene
                        .objects
                        .get(mask)
                        .map_or("the analysis mask", |set| set.name());
                    let set = self.name(id);
                    return Err(format!(
                        "the set {set} is masked by {name}, which detected no objects"
                    ));
                }
                self.scene.mask_frames()?
            }
        };
        let object = self
            .scene
            .objects
            .get(id)
            .expect("the staged object set exists");
        let name = object.name().to_string();
        let channel = object.channel();
        let params = object.params();
        let segmented = object.provenance().segment().cloned();
        let seeds = self.scene.seed_frames(id);
        let policy = match self.scene.seed_source(id) {
            Some(_) => crate::SeedPolicy::Sourced,
            None if seeds.iter().any(|frame| !frame.is_empty()) => crate::SeedPolicy::Strict,
            None => crate::SeedPolicy::Unseeded,
        };
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("detecting {name}"),
        });
        let reading = Reading {
            reader: &self.channels,
            channel,
            region: self.scene.region,
            roi,
            drift: self.scene.drift().clone(),
        };
        let frames = match segmented {
            Some(params) => segment_frames(reading, params, seeds, &self.progress)?,
            None => detect(
                task,
                reading,
                params,
                seeds,
                policy,
                self.store,
                &self.progress,
            )?,
        };
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                self.scene.install_shared(id, time as u32, Arc::new(frame));
            }
        }
        self.scene.apply_curation(id);
        self.track(id)
    }

    fn import(&mut self, id: ObjectSetId) -> Result<(), String> {
        let task = self.task;
        let provenance = self
            .scene
            .objects
            .get(id)
            .map(|set| set.provenance().clone())
            .ok_or_else(|| "the object set left the scene".to_string())?;
        let label = match provenance {
            grana_scene::Provenance::Imported { label } => label,
            grana_scene::Provenance::Interop { .. } | grana_scene::Provenance::Attached { .. } => {
                return Err(
                    "imported tracks belong to a single dataset and cannot be batched".to_string(),
                );
            }
            grana_scene::Provenance::Detection
            | grana_scene::Provenance::Segmented { .. }
            | grana_scene::Provenance::Filtered { .. }
            | grana_scene::Provenance::Copied { .. }
            | grana_scene::Provenance::Edited { .. }
            | grana_scene::Provenance::Masked { .. }
            | grana_scene::Provenance::Merged { .. } => {
                return Err("the object set names no label image".to_string());
            }
        };
        let name = self.name(id);
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("importing {name}"),
        });
        let frames = self.scene.frame_count() as usize;
        let mut collected = vec![None; frames];
        let mut failure = None;
        let progress = &self.progress;
        let mut sink = |event: RunEvent| match event {
            RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
            RunEvent::Failed(detail) => failure = Some(detail),
            RunEvent::Progress { done, total, .. } if progress.hold() => {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
            RunEvent::Progress { .. } => {}
            RunEvent::Tracks(_)
            | RunEvent::Relation { .. }
            | RunEvent::Coloc { .. }
            | RunEvent::ObjectColoc { .. }
            | RunEvent::Drift(_)
            | RunEvent::Calibration { .. }
            | RunEvent::Done => {}
        };
        crate::run::import(
            &self.channels,
            crate::run::ImportRequest::new(&label, self.scene.drift().clone()),
            task.cancel,
            &mut sink,
        )?;
        if let Some(detail) = failure {
            return Err(detail);
        }
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        for (time, frame) in collected.into_iter().enumerate() {
            if let Some(frame) = frame {
                self.scene.install_shared(id, time as u32, Arc::new(frame));
            }
        }
        self.scene.apply_curation(id);
        self.track(id)
    }

    fn refilter(&mut self, id: ObjectSetId) -> Result<(), String> {
        let name = self.name(id);
        let _ = self.task.events.send(Event::Step {
            index: self.task.index,
            label: format!("filtering {name}"),
        });
        self.scene.refresh_filtered(id)?;
        self.scene.clear_tracks(id);
        self.track(id)
    }

    fn copy(&mut self, id: ObjectSetId, source: ObjectSetId) -> Result<(), String> {
        let frames = (0..self.scene.frame_count())
            .map(|time| {
                self.scene
                    .objects
                    .get(source)
                    .and_then(|set| set.frame_shared(time))
            })
            .collect::<Vec<_>>();
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                self.scene.install_shared(id, time as u32, frame);
            }
        }
        self.scene.apply_curation(id);
        Ok(())
    }

    fn merge(&mut self, id: ObjectSetId, part: ObjectSetId) -> Result<(), String> {
        self.scene.merge_objects(part, id)?;
        self.scene.apply_curation(id);
        Ok(())
    }

    fn edit(&mut self, id: ObjectSetId) -> Result<(), String> {
        let name = self.name(id);
        let _ = self.task.events.send(Event::Step {
            index: self.task.index,
            label: format!("editing {name}"),
        });
        let (source, provenance, voxel, channel) = self
            .scene
            .objects
            .get(id)
            .and_then(|object| {
                object.provenance().source().map(|source| {
                    (
                        source,
                        object.provenance().clone(),
                        object.voxel(),
                        object.channel(),
                    )
                })
            })
            .ok_or_else(|| format!("{name} carries no edits"))?;
        let frames = (0..self.scene.frame_count())
            .map(|time| {
                self.scene
                    .objects
                    .get(source)
                    .and_then(|object| object.frame_shared(time))
            })
            .collect::<Vec<_>>();
        let edited = match &provenance {
            grana_scene::Provenance::Edited { edits, .. } => {
                crate::run::edit(&frames, edits, voxel, Some(self.store))?
            }
            grana_scene::Provenance::Masked { strokes, .. } => {
                let level = frames
                    .iter()
                    .flatten()
                    .find_map(|frame| match frame.as_ref() {
                        ObjectFrame::Surfaces(set) => Some(set.level()),
                        _ => None,
                    });
                let Some(level) = level else {
                    return Err(format!("{name} carries no surface frame"));
                };
                let request = crate::run::MaskRequest {
                    channel,
                    level,
                    strokes,
                    drift: self.scene.drift(),
                };
                crate::run::mask(&self.channels, &frames, voxel, &request)?
            }
            _ => return Err(format!("{name} carries no edits")),
        };
        for (time, frame) in edited.into_iter().enumerate() {
            match frame {
                Some(frame) => self.scene.install_shared(id, time as u32, frame),
                None => self.scene.install_optional(id, time as u32, None),
            }
        }
        self.scene.clear_tracks(id);
        self.track(id)
    }

    fn name(&self, id: ObjectSetId) -> String {
        self.scene
            .objects
            .get(id)
            .map_or_else(String::new, |set| set.name().to_string())
    }

    fn track(&mut self, id: ObjectSetId) -> Result<(), String> {
        let Some(object) = self.scene.objects.get(id) else {
            return Ok(());
        };
        let name = object.name().to_string();
        if self.scene.frame_count() < 2 || !object.kind().trackable() || object.detected() == 0 {
            return Ok(());
        }
        let params = object
            .track_style()
            .expect("a trackable object carries track parameters")
            .params();
        let mut detections = vec![Vec::new(); self.scene.frame_count() as usize];
        for (time, frame) in object.frames() {
            detections[time as usize] = frame.centroids();
        }
        let frame_interval = self.scene.dataset.frame_interval();
        let task = self.task;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("tracking {name}"),
        });
        let mut tracks = None;
        let mut sink = |event: RunEvent| {
            if let RunEvent::Tracks(set) = event {
                tracks = Some(set);
            }
        };
        run::tracks(detections, params, frame_interval, task.cancel, &mut sink);
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        if let Some(tracks) = tracks {
            self.scene.install_tracks(id, tracks);
        }
        Ok(())
    }

    fn relate(
        &mut self,
        source_id: ObjectSetId,
        target_id: ObjectSetId,
        shift: i32,
    ) -> Result<(), String> {
        let task = self.task;
        let source_name = self
            .scene
            .objects
            .get(source_id)
            .expect("the staged relation source exists")
            .name()
            .to_string();
        let target_name = self
            .scene
            .objects
            .get(target_id)
            .expect("the staged relation target exists")
            .name()
            .to_string();
        let source_frames = frames_of(&self.scene, source_id);
        let target_frames = frames_of(&self.scene, target_id);
        let gate = self.scene.relation_gate(source_id, target_id);
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("relating {source_name} to {target_name}"),
        });
        let progress = &self.progress;
        let scene = &mut self.scene;
        let mut failure = None;
        let mut sink = |event: RunEvent| match event {
            RunEvent::Relation { time, frame } => {
                scene.install_relation(source_id, target_id, shift, time, frame);
            }
            RunEvent::Failed(detail) => failure = Some(detail),
            RunEvent::Progress { done, total, .. } if progress.hold() => {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
            _ => {}
        };
        run::relation(
            source_frames,
            target_frames,
            run::RelationRequest {
                shift,
                same: source_id == target_id,
                gate,
                workers: task.workers,
            },
            task.cancel,
            &mut sink,
        );
        if let Some(detail) = failure {
            return Err(detail);
        }
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        Ok(())
    }

    fn label(&mut self, set: ObjectSetId) -> Result<(), String> {
        let task = self.task;
        let dataset = self.dataset;
        let object = self
            .scene
            .objects
            .get(set)
            .ok_or_else(|| "the labelled object set left the scene".to_string())?;
        let name = object.name().to_string();
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("writing the label image of {name}"),
        });
        if self.scene.dataset.origin() != grana_scene::Origin::Zarr {
            return Err("label images are written into local OME-Zarr datasets".to_string());
        }
        let frames = (0..self.scene.frame_count())
            .filter(|time| object.frame(*time).is_some())
            .collect::<Vec<_>>();
        let labels = frames
            .iter()
            .filter_map(|time| object.frame(*time).map(ObjectFrame::object_count))
            .max()
            .unwrap_or(0);
        if labels == 0 {
            return Err(format!("{name} holds no objects to label"));
        }
        if labels > u16::MAX as usize {
            return Err(format!(
                "a uint16 label image addresses at most {} objects",
                u16::MAX
            ));
        }
        let shapes = (0..self.scene.frame_count())
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        let visible = (0..self.scene.frame_count())
            .map(|time| {
                object.frame(time).map_or_else(Vec::new, |frame| {
                    (0..frame.object_count())
                        .map(|index| object.visible_object(time, index))
                        .collect()
                })
            })
            .collect::<Vec<_>>();
        let levels =
            self.scene.dataset.pyramid().levels()[..self.scene.dataset.stored_levels()].to_vec();
        if levels.is_empty() {
            return Err("the dataset holds no stored resolution level".to_string());
        }
        let mut labeling = Labeling::new(shapes, visible, labels, levels, name.clone());
        let properties = labeling.properties();
        let cancel = task.cancel;
        let mut fill =
            |level: usize, time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".to_string());
                }
                labeling.fill(level, time, origin, extent, data)
            };
        let events = task.events;
        let index = task.index;
        let mut progress = move |done: usize, total: usize| {
            let _ = events.send(Event::Progress { index, done, total });
        };
        let group = grana_zarr::group_path(Path::new(dataset.group()));
        let written = grana_zarr::write_group_labels(
            dataset.path(),
            &group,
            grana_zarr::LabelSpec {
                name: &slug(&name),
                frames: &frames,
                labels,
                color: object.color(),
                opacity: object.opacity(),
                properties: &properties,
                fill: &mut fill,
                progress: &mut progress,
            },
        )
        .map_err(|error| error.to_string())?;
        self.scene.set_label_export(
            set,
            Some(grana_scene::LabelExport::new(
                written.path().display().to_string(),
                written.frames() as u32,
                written.labels() as u32,
            )),
        );
        Ok(())
    }

    fn colocalize(
        &mut self,
        pair: usize,
        mask: Option<ObjectSetId>,
        mask_radius: f32,
        filter: Option<MaskFilter>,
    ) -> Result<(), String> {
        let task = self.task;
        let coloc = task
            .protocol
            .coloc()
            .expect("a colocalization step names its spec");
        let spec = &coloc.pairs[pair];
        let settings = coloc.settings();
        let pair = self.pairs[pair];
        self.scene.set_mask(mask);
        self.scene.set_mask_radius(mask_radius);
        self.scene.set_mask_filter(filter);
        self.scene.coloc.set_settings(settings);
        self.scene.coloc.set_pair(pair, self.scene.channels.len());
        let roi = match self.scene.mask {
            None => None,
            Some(mask) => {
                let object = self.scene.objects.get(mask);
                if !object.is_some_and(|set| set.detected() > 0) {
                    let name = object.map_or("the colocalization mask", |set| set.name());
                    return Err(format!("{name} detected no objects"));
                }
                self.scene.mask_frames()?
            }
        };
        let region = self.scene.region;
        let mask_id = self.scene.mask.map(|mask| mask.raw());
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("colocalizing {} with {}", spec.a, spec.b),
        });
        let progress = &self.progress;
        let mut series: Option<Box<ColocSeries>> = None;
        let mut sink = |event: RunEvent| match event {
            RunEvent::Coloc { series: found, .. } => series = Some(found),
            RunEvent::Progress { done, total, .. } if progress.hold() => {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
            _ => {}
        };
        run::coloc(
            &self.channels,
            run::ColocRequest::new(
                pair,
                region,
                roi,
                mask_id,
                settings,
                self.scene.drift().clone(),
                task.workers,
            ),
            task.cancel,
            &mut sink,
        )?;
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let series = series.ok_or_else(|| {
            format!(
                "the colocalization of {} with {} produced no result",
                spec.a, spec.b
            )
        })?;
        self.scene.coloc.install(*series);
        self.scene
            .coloc
            .set_thresholds(grana_measure::ColocThresholds::new(
                spec.thresholds[0],
                spec.thresholds[1],
            ));
        Ok(())
    }
    fn object_coloc(&mut self, set: ObjectSetId, pair: ColocPair) -> Result<(), String> {
        let task = self.task;
        let name = self.name(set);
        if self
            .scene
            .objects
            .get(set)
            .is_none_or(|object| object.detected() == 0)
        {
            return Err(format!("{name} detected no objects to colocalize"));
        }
        let frames = frames_of(&self.scene, set);
        let region = self.scene.region;
        if region.enabled()
            && region
                .voxel_range(self.channels.pyramid().level(0))
                .is_none()
        {
            return Err("the region does not overlap the dataset".to_string());
        }
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("colocalizing the objects of {name}"),
        });
        let progress = &self.progress;
        let mut found: Option<Box<grana_measure::ObjectColoc>> = None;
        let mut failure = None;
        let mut sink = |event: RunEvent| match event {
            RunEvent::ObjectColoc { coloc } => found = Some(coloc),
            RunEvent::Failed(detail) => failure = Some(detail),
            RunEvent::Progress { done, total, .. } if progress.hold() => {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
            _ => {}
        };
        crate::run::object_coloc(
            &self.channels,
            crate::run::ObjectColocRequest::new(
                pair,
                region,
                frames,
                self.scene.drift().clone(),
                task.workers,
            ),
            task.cancel,
            &mut sink,
        )?;
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        if let Some(detail) = failure {
            return Err(detail);
        }
        let coloc = found
            .ok_or_else(|| format!("the object colocalization of {name} produced no result"))?;
        self.scene.install_object_coloc(set, *coloc);
        Ok(())
    }
}

fn frames_of(scene: &Scene, id: ObjectSetId) -> Vec<Option<Arc<ObjectFrame>>> {
    (0..scene.frame_count())
        .map(|time| scene.objects.get(id).and_then(|set| set.frame_shared(time)))
        .collect()
}

fn capture_view(
    capture: &dyn Capture,
    render: &Channels,
    scene: &Scene,
    spec: &SnapshotSpec,
    view: ViewState,
    folder: &Path,
    stem: &str,
) -> Result<(), String> {
    let still_size = spec.size_for(Some(view.export));
    let film_size = spec.film_size_for(Some(view.export));
    let still = CaptureRequest {
        view: view.clone(),
        size: still_size,
        transparent: spec.transparent,
        kind: CaptureKind::Still { time: scene.time() },
    };
    capture.capture(
        render,
        scene,
        &still,
        &folder.join(format!("snapshot{stem}.png")),
    )?;
    if spec.movies() {
        let film = CaptureRequest {
            view: view.clone(),
            size: film_size,
            transparent: spec.transparent,
            kind: CaptureKind::Movie,
        };
        capture.capture(
            render,
            scene,
            &film,
            &folder.join(format!("movie{stem}.mp4")),
        )?;
    }
    if spec.sequences() {
        let frames = CaptureRequest {
            view,
            size: still_size,
            transparent: spec.transparent,
            kind: CaptureKind::Sequence,
        };
        capture.capture(
            render,
            scene,
            &frames,
            &folder.join(format!("sequence{stem}")),
        )?;
    }
    Ok(())
}

fn capture_stems(protocol: &Protocol) -> Vec<String> {
    let Some(snapshot) = protocol.snapshot() else {
        return Vec::new();
    };
    if snapshot.views.is_empty() {
        return vec![String::new()];
    }
    snapshot
        .views
        .iter()
        .enumerate()
        .map(|(index, name)| format!("-{:02}-{}", index + 1, slug(name)))
        .collect()
}

pub(super) fn capture_files(
    protocol: &Protocol,
    prefix: &str,
    extension: Option<&str>,
) -> Vec<String> {
    capture_stems(protocol)
        .into_iter()
        .map(|stem| match extension {
            Some(extension) => format!("{prefix}{stem}.{extension}"),
            None => format!("{prefix}{stem}"),
        })
        .collect()
}

pub(super) fn artifacts(
    protocol: &Protocol,
    name: &str,
    project: bool,
    channels: bool,
) -> Vec<String> {
    let mut artifacts = protocol
        .sets()
        .iter()
        .map(|set| format!("{}-{}.csv", set.key(), set.group().key()))
        .collect::<Vec<_>>();
    for relation in protocol.relations() {
        artifacts.push(grana_measure::PairExport::file_name(
            &relation.source,
            &relation.target,
            relation.shift,
        ));
    }
    for (index, annotation) in protocol.annotations().iter().enumerate() {
        if annotation.profile().is_some() {
            let stem = format!("profile-{index:02}-{}", slug(annotation.name()));
            artifacts.push(format!("{stem}.csv"));
            artifacts.push(format!("{stem}.png"));
        }
    }
    if let Some(coloc) = protocol.coloc() {
        for pair in &coloc.pairs {
            let stem = format!("coloc-{}-{}", slug(&pair.a), slug(&pair.b));
            artifacts.push(format!("{stem}.csv"));
            artifacts.push(format!("{stem}.png"));
        }
    }
    if channels {
        artifacts.push(format!("{}.ome.zarr", slug(name)));
    }
    if let Some(snapshot) = protocol.snapshot() {
        artifacts.extend(capture_files(protocol, "snapshot", Some("png")));
        if snapshot.movies() {
            artifacts.extend(capture_files(protocol, "movie", Some("mp4")));
        }
        if snapshot.sequences() {
            artifacts.extend(capture_files(protocol, "sequence", None));
        }
    }
    if project {
        artifacts.push(format!("{}.granaproj", slug(name)));
    }
    artifacts
}

fn dataset_of(format: Option<Format>, name: &str, pyramid: &Pyramid) -> Dataset {
    let pyramid = pyramid.clone();
    match format {
        Some(Format::Zarr) => Dataset::zarr(name, pyramid),
        Some(Format::Tiff) => Dataset::tiff(name, pyramid),
        Some(Format::Czi) => Dataset::czi(name, pyramid),
        Some(Format::Nikon) => Dataset::nikon(name, pyramid),
        Some(Format::Leica) => Dataset::leica(name, pyramid),
        Some(Format::Imaris) => Dataset::imaris(name, pyramid),
        None => Dataset::synthetic(name, pyramid),
    }
}

fn view_state(view: Option<&ViewSpec>, scene: &Scene) -> ViewState {
    let duration = (scene.frame_count() as f32 * scene.dataset.frame_interval())
        .clamp(1.0, Timeline::MAX_DURATION);
    let fitted = || Timeline::new(30, duration);
    match view {
        Some(view) => ViewState {
            panes: view.panes(),
            layout: view.layout(),
            split: view.split,
            row: view.row,
            timeline: view.timeline().unwrap_or_else(fitted),
            export: view.export().unwrap_or(Export::NATIVE),
        },
        None => {
            let mut panes = grana_display::Panes::new();
            panes.fit(
                scene.dataset.world_size(),
                scene.dataset.voxel_size(),
                [1.6; grana_display::PANE_COUNT],
            );
            ViewState {
                panes: panes.states(),
                layout: panes.layout(),
                split: panes.split(),
                row: panes.row(),
                timeline: fitted(),
                export: Export::NATIVE,
            }
        }
    }
}

struct Reading<'a> {
    reader: &'a Channels,
    channel: usize,
    region: Region,
    roi: Option<Roi>,
    drift: grana_scene::Drift,
}

fn detect(
    task: &Task<'_>,
    reading: Reading<'_>,
    params: ObjectParams,
    seeds: Vec<Vec<[f32; 3]>>,
    policy: grana_analysis::SeedPolicy,
    store: &Arc<GeometryStore>,
    progress: &Progress<'_>,
) -> Result<Vec<Option<ObjectFrame>>, String> {
    let frames = reading.reader.pyramid().frame_count() as usize;
    let mut collected = vec![None; frames];
    let mut sink = |event: RunEvent| match event {
        RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
        RunEvent::Progress { done, total, .. } if progress.hold() => {
            let _ = progress.events.send(Event::Progress {
                index: progress.index,
                done,
                total,
            });
        }
        RunEvent::Progress { .. } => {}
        RunEvent::Tracks(_)
        | RunEvent::Relation { .. }
        | RunEvent::Coloc { .. }
        | RunEvent::ObjectColoc { .. }
        | RunEvent::Drift(_)
        | RunEvent::Calibration { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::detect(
        reading.reader,
        run::DetectRequest::new(
            reading.channel,
            params,
            reading.region,
            reading.roi,
            reading.drift,
            task.workers,
            Arc::clone(store),
        )
        .with_seeds(seeds, policy),
        progress.cancel,
        &mut sink,
    )?;
    Ok(collected)
}

fn segment_frames(
    reading: Reading<'_>,
    params: grana_scene::SegmentParams,
    seeds: Vec<Vec<[f32; 3]>>,
    progress: &Progress<'_>,
) -> Result<Vec<Option<ObjectFrame>>, String> {
    let frames = reading.reader.pyramid().frame_count() as usize;
    let mut collected = vec![None; frames];
    let mut sink = |event: RunEvent| match event {
        RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
        RunEvent::Progress { done, total, .. } if progress.hold() => {
            let _ = progress.events.send(Event::Progress {
                index: progress.index,
                done,
                total,
            });
        }
        RunEvent::Progress { .. } => {}
        RunEvent::Tracks(_)
        | RunEvent::Relation { .. }
        | RunEvent::Coloc { .. }
        | RunEvent::ObjectColoc { .. }
        | RunEvent::Drift(_)
        | RunEvent::Calibration { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::segment(
        reading.reader,
        run::SegmentRequest::new(
            reading.channel,
            params,
            reading.region,
            reading.drift,
            reading.roi,
        )
        .with_seeds(seeds),
        progress.cancel,
        &mut sink,
    )?;
    Ok(collected)
}

fn clear(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(path)
        .map_err(|error| format!("failed to clear {}: {error}", path.display()))
}

fn colocalization(
    fonts: &mut grana_text::Fonts,
    result: &grana_scene::ColocResult,
    x: &str,
    y: &str,
    x_tint: [f32; 3],
    y_tint: [f32; 3],
) -> grana_raster::Raster {
    let series = result.series();
    let thresholds = result.thresholds();
    let statistics = result.statistics();
    let manders = statistics.manders();
    let costes = series.costes().map(|range| {
        let low = range.low();
        let high = range.high();
        [[low.a(), low.b()], [high.a(), high.b()]]
    });
    let caption = format!(
        "Pearson {} · Overlap {} · M1 {} · M2 {} · p {}",
        number(statistics.pearson()),
        number(statistics.overlap()),
        number(manders[0]),
        number(manders[1]),
        statistics.significance().map_or_else(
            || "\u{2014}".to_string(),
            |significance| format!("{:.4}", significance.p())
        ),
    );
    let subtitle = format!(
        "all frames · {} bins · level {}",
        series.bins(),
        series.level()
    );
    let plot = grana_raster::Joint {
        x: grana_raster::Axis::new(x, x_tint),
        y: grana_raster::Axis::new(y, y_tint),
        bins: series.bins(),
        joint: series.joint(),
        threshold: [thresholds.a(), thresholds.b()],
        costes,
        title: "Colocalization",
        subtitle: &subtitle,
        caption: &caption,
    };
    grana_raster::joint_plot(fonts, &plot, COLOC_PLOT)
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "\u{2014}".to_string(), |value| format!("{value:.3}"))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}
