use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_animation::Timeline;
use grana_display::Camera;
use grana_image::{Format, Image, Source};
use grana_measure::{ColocPair, ColocSeries, Group, Lasso};
use grana_mesh::Format as MeshFormat;
use grana_object::Roi;
use grana_project::{Export, NamedView, Project, Source as ProjectSource, ViewState};
use grana_scene::{
    Dataset, MaskFilter, Measure, ObjectFrame, ObjectParams, ObjectSetId, Scene, slug,
};
use grana_store::DEFAULT_BUDGET;
use grana_store::SpillStore;
use grana_volume::{Pyramid, Region};

use crate::capture::{Capture, CaptureRequest, Kind as CaptureKind};
use crate::channels::Channels;
use crate::event::Event as RunEvent;
use crate::label::Labeling;
use crate::mesh::{self, Export as MeshExport};
use crate::run;
use grana_protocol::{Protocol, ProtocolError, SnapshotSpec, SourceSpec, Step, ViewSpec};

use super::channel;
use super::profile;
use super::resume::{Record, build};
use super::{DatasetEntry, Event, PARTIAL, Progress, RESULT, Task};

pub(super) struct Report {
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<grana_report::Quality>,
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
    let path = dataset.image_path();
    let scene = dataset.image_scene();
    let source = match grana_image::series_group(path, scene)? {
        Some(group) => Source::file_group(path.to_path_buf(), scene, group),
        None => Source::file_series(path.to_path_buf(), scene),
    };
    let reader = Arc::new(Image::open(&source)?);
    let store = SpillStore::temporary(DEFAULT_BUDGET)
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
        folder,
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
                warps,
            } => run.align(*channel, *reference, warps)?,
            Step::Register { reference, warps } => run.register(*reference, warps)?,
            Step::Detect {
                set,
                mask,
                mask_radius,
                filter,
            } => run.detect(*set, *mask, *mask_radius, *filter)?,
            Step::Import { set } => run.import(*set)?,
            Step::Localize { set, molecules } => run.localize(*set, molecules)?,
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
    if let Some(composition) = task.protocol.composition() {
        run.compose(&run.sets, composition)?;
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
    let keys = grana_report::keys(
        &scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>(),
    );
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
        stream(&folder.join(&file), &table, &rows)?;
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
        let object = scene
            .objects
            .get(sets[index])
            .expect("the batch measures its detected set");
        let compare = spec.measure().map_or(&[][..], |measure| measure.compare());
        quality.push(grana_report::quality_of(
            &table,
            &rows,
            &grana_report::QualityInput {
                dataset: &name,
                group: dataset.group(),
                key: spec.key(),
                name: spec.name.as_str(),
                kind: object.kind().name(),
                channel: object.channel(),
            },
            &keys,
            compare,
        )?);
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
    let mut pages = Vec::new();
    if scene
        .annotations
        .iter()
        .any(|annotation| annotation.profile().is_some())
    {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("exporting the intensity profiles of {}", reader.name()),
        });
        pages.extend(profile::write(&scene, folder, language())?);
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
        for result in scene.coloc.results() {
            let pair = result.pair();
            let stem = format!(
                "coloc-{}-{}",
                slug(&channels[pair.a()]),
                slug(&channels[pair.b()])
            );
            write(
                &folder.join(format!("{stem}.csv")),
                &result.series().csv(result.thresholds()),
            )?;
            let page = colocalization(
                result,
                &channels[pair.a()],
                &channels[pair.b()],
                [tints[pair.a()], tints[pair.b()]],
                &name,
                language(),
            );
            grana_page::write_page(&page, &folder.join(format!("{stem}.svg")))?;
            pages.push(page);
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
    let mut recorded = Vec::new();
    if let Some(spec) = task.protocol.snapshot() {
        for (index, stem) in capture_stems(task.protocol).iter().enumerate() {
            let file = folder.join(format!("snapshot{stem}.png"));
            if !file.is_file() {
                continue;
            }
            let mut detail = format!("{} · {}", dataset.group(), dataset.scene_label());
            if let Some(view) = spec.views().get(index) {
                detail.push_str(&format!(" · {view}"));
            }
            recorded.push(grana_page::Recorded::picture(
                &file,
                &dataset.title(),
                &detail,
            )?);
        }
    }
    recorded.extend(pages.iter().map(grana_page::Recorded::of));
    grana_page::Recorded::write(&folder.join(super::PAGES), &recorded)?;
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
        let file = format!("{}.grana", slug(&name));
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
    store: &'b Arc<SpillStore>,
    folder: &'b Path,
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
        warps: &[grana_warp::Warp],
    ) -> Result<(), String> {
        let task = self.task;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("aligning the frames of {}", self.channels.name()),
        });
        self.scene
            .set_drift(grana_scene::Drift::new(channel, reference, warps.to_vec()));
        Ok(())
    }

    fn register(&mut self, reference: usize, warps: &[grana_warp::Warp]) -> Result<(), String> {
        let task = self.task;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!(
                "registering the channels of {}",
                self.scene.dataset.channels()[reference].name()
            ),
        });
        self.scene
            .set_registration(grana_scene::Registration::new(reference, warps.to_vec()));
        let reader = self.channels.image_shared();
        self.channels = Channels::analysis(reader, &self.scene);
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
        let seed_source = self.scene.seed_source(id);
        let sources = seed_source.map(|source| {
            let nuclei = self
                .scene
                .objects
                .get(source)
                .expect("the staged seed source exists");
            (0..nuclei.frame_count())
                .map(|time| nuclei.frame_shared(time))
                .collect::<Vec<_>>()
        });
        let policy = match seed_source {
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
            drifts: self.scene.drifts().clone(),
        };
        let frames = match (segmented, params) {
            (Some(params), _) => segment_frames(reading, params, seeds, &self.progress)?,
            (None, ObjectParams::Cells(params)) => {
                let sources = sources
                    .ok_or_else(|| format!("the cells {name} grow from no nucleus object set"))?;
                grow_frames(reading, params, sources, &self.progress)?
            }
            (None, params) => detect(
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
        let provenance = self
            .scene
            .objects
            .get(id)
            .map(|set| set.provenance().clone())
            .ok_or_else(|| "the object set left the scene".to_string())?;
        match provenance {
            grana_scene::Provenance::Imported { label } => self.import_labels(id, &label)?,
            grana_scene::Provenance::Interop { file }
                if grana_readout::localization::has_extension(Path::new(&file)) =>
            {
                self.import_localizations(id, &file)?;
            }
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
        }
        self.scene.apply_curation(id);
        let carried = self
            .scene
            .objects
            .get(id)
            .is_some_and(|set| set.tracks().is_some());
        if carried {
            return Ok(());
        }
        self.track(id)
    }

    fn import_labels(&mut self, id: ObjectSetId, label: &str) -> Result<(), String> {
        let task = self.task;
        let name = self.name(id);
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("importing {name}"),
        });
        let reader = &self.channels;
        let drifts = self.scene.drifts().clone();
        let channel = self.scene.objects.get(id).map_or(0, |set| set.channel());
        let frames = self.scene.frame_count() as usize;
        let progress = &self.progress;
        let mut collected = drain(frames, progress, &mut |_| {}, |sink| {
            crate::run::import(
                reader,
                crate::run::ImportRequest::new(channel, label, drifts.clone()),
                task.cancel,
                sink,
            )
        })?;
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        for (time, frame) in collected.iter_mut().enumerate() {
            if let Some(frame) = frame.take() {
                self.scene.install_shared(id, time as u32, Arc::new(frame));
            }
        }
        Ok(())
    }

    fn import_localizations(&mut self, id: ObjectSetId, file: &str) -> Result<(), String> {
        let task = self.task;
        let name = self.name(id);
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("importing the localizations of {name}"),
        });
        let reader = &self.channels;
        let alignment = self.alignment(id)?;
        let frames = self.scene.frame_count() as usize;
        let progress = &self.progress;
        let mut localization = None;
        let mut collected = drain(
            frames,
            progress,
            &mut |event| {
                if let RunEvent::Localization {
                    pixel,
                    width,
                    height,
                    volume,
                    photons,
                    alignment,
                } = event
                {
                    localization = Some((pixel, width, height, volume, photons, alignment));
                }
            },
            |sink| {
                crate::run::localization(
                    reader,
                    crate::run::LocalizationRequest::new(file, alignment),
                    task.cancel,
                    sink,
                )
            },
        )?;
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        for (time, frame) in collected.iter_mut().enumerate() {
            if let Some(frame) = frame.take() {
                self.scene.install_shared(id, time as u32, Arc::new(frame));
            }
        }
        let (pixel, width, height, volume, photons, alignment) = localization
            .ok_or_else(|| format!("the localization table of {name} holds no localization"))?;
        let spec = grana_object::LocalizationSpec::cover(self.scene.frame_count());
        self.scene.set_localization(
            id,
            grana_scene::LocalizationSource::new(
                pixel, width, height, volume, photons, spec, alignment,
            ),
        );
        self.scene.rebuild_molecule_tracks(id)?;
        Ok(())
    }

    fn alignment(&self, id: ObjectSetId) -> Result<grana_scene::LocalizationAlignment, String> {
        let channel = self
            .scene
            .objects
            .get(id)
            .ok_or_else(|| "the object set left the scene".to_string())?
            .channel();
        Ok(grana_scene::LocalizationAlignment::new(
            channel,
            self.scene.drift(channel).cloned().unwrap_or_default(),
            self.scene.registration().clone(),
        ))
    }

    fn localize(
        &mut self,
        id: ObjectSetId,
        molecules: &grana_protocol::MoleculesSpec,
    ) -> Result<(), String> {
        let filter = molecules.filter();
        if filter.photons() > 0.0 {
            let supported = self
                .scene
                .objects
                .get(id)
                .and_then(|object| object.localization())
                .is_some_and(|source| source.photons());
            if !supported {
                return Err(format!(
                    "the localization table of {} carries no photon count to filter",
                    self.name(id)
                ));
            }
        }
        self.scene
            .set_localization_spec(id, molecules.localization());
        self.scene
            .set_localization_clusters(id, molecules.clusters());
        self.scene
            .set_localization_group_colored(id, molecules.group_colored);
        self.scene
            .set_localization_projected(id, molecules.projected);
        if molecules.clustered {
            self.cluster(id, molecules.clusters())?;
        }
        self.superres(id, molecules)
    }

    fn superres(
        &self,
        id: ObjectSetId,
        molecules: &grana_protocol::MoleculesSpec,
    ) -> Result<(), String> {
        let spec = molecules.localization().fitted(self.scene.frame_count());
        let Some(object) = self.scene.objects.get(id) else {
            return Err("the localization set left the scene".to_string());
        };
        let Some(source) = object.localization() else {
            return Err(format!(
                "{} carries no localization table to render",
                object.name()
            ));
        };
        let name = object.name().to_string();
        let key = object.key().as_str().to_string();
        let window = run::LocalizationWindow::of(object, spec.frames());
        let pixel = source.pixel() * 0.001;
        let path = self.folder.join(format!("{key}-superres.ome.tif"));
        let _ = self.task.events.send(Event::Step {
            index: self.task.index,
            label: format!("rendering the super-resolution image of {name}"),
        });
        let request = run::LocalizationImageRequest::new(window, spec, pixel, &name, 0, &path)?;
        let progress = &self.progress;
        run::localization_image(request, self.task.cancel, &mut |event| {
            if let RunEvent::Progress { done, total, .. } = event
                && progress.hold()
            {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
        })
    }

    fn compose(
        &self,
        sets: &[ObjectSetId],
        composition: &grana_protocol::CompositionSpec,
    ) -> Result<(), String> {
        let mut sources = Vec::with_capacity(composition.sets.len());
        for key in &composition.sets {
            let index = self
                .task
                .protocol
                .sets()
                .iter()
                .position(|spec| &spec.key == key)
                .ok_or_else(|| format!("the composition names the missing object set {key}"))?;
            let object = self
                .scene
                .objects
                .get(sets[index])
                .ok_or_else(|| format!("the object set {key} left the scene"))?;
            let source = object.localization().ok_or_else(|| {
                format!("the composition names {key}, which carries no localization table")
            })?;
            let spec = source.spec().fitted(self.scene.frame_count());
            sources.push(run::CompositionSource::new(
                run::LocalizationWindow::of(object, spec.frames()),
                spec,
                object.color(),
            ));
        }
        let name = self.scene.dataset.name().to_string();
        let path = self.folder.join(grana_protocol::CompositionSpec::FILE);
        let request = run::LocalizationCompositionRequest::new(sources, &name, &path)?;
        let _ = self.task.events.send(Event::Step {
            index: self.task.index,
            label: format!("composing the super-resolution image of {name}"),
        });
        let progress = &self.progress;
        run::localization_composition(request, self.task.cancel, &mut |event| {
            if let RunEvent::Progress { done, total, .. } = event
                && progress.hold()
            {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
        })
    }

    fn cluster(&mut self, id: ObjectSetId, spec: grana_object::ClusterSpec) -> Result<(), String> {
        let task = self.task;
        let name = self.name(id);
        let Some(object) = self.scene.objects.get(id) else {
            return Err(format!("{name} left the scene before it clustered"));
        };
        let grouped = object
            .localization()
            .is_none_or(|source| !source.clustered());
        let filter = object
            .localization()
            .map_or(grana_object::LocalizationFilter::NONE, |source| {
                source.spec().filter()
            });
        let revision = object.revision();
        let window = run::LocalizationWindow::of(object, [0, self.scene.frame_count() - 1]);
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("clustering the localizations of {name}"),
        });
        let progress = &self.progress;
        let mut found = None;
        let mut failure = None;
        let mut sink = |event: RunEvent| match event {
            RunEvent::LocalizationClusters { clusters, .. } => found = Some(clusters),
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
        crate::run::localization_cluster(
            crate::run::LocalizationClusterRequest::new(
                id, revision, window, spec, filter, grouped,
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
        let clusters =
            found.ok_or_else(|| format!("the localizations of {name} produced no molecule"))?;
        self.scene
            .install_molecules(id, clusters.groups())
            .map(|_| ())
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
                    drifts: self.scene.drifts(),
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
            detections[time as usize] = frame.detections();
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
            .filter(|time| object.frame_stamp(*time).is_some())
            .collect::<Vec<_>>();
        let labels = frames
            .iter()
            .map(|time| object.object_count(*time))
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
                {
                    (0..object.object_count(time))
                        .map(|index| object.visible_object(time, index))
                        .collect()
                }
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
                self.scene.drifts().clone(),
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
                self.scene.drifts().clone(),
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

fn drain(
    frames: usize,
    progress: &Progress<'_>,
    extra: &mut dyn FnMut(RunEvent),
    run: impl FnOnce(&mut dyn FnMut(RunEvent)) -> Result<(), String>,
) -> Result<Vec<Option<ObjectFrame>>, String> {
    let mut collected = vec![None; frames];
    let mut failure = None;
    {
        let mut sink = |event: RunEvent| match event {
            RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
            RunEvent::Failed(detail) => failure = Some(detail),
            RunEvent::Done => {}
            RunEvent::Progress { done, total, .. } => {
                if progress.hold() {
                    let _ = progress.events.send(Event::Progress {
                        index: progress.index,
                        done,
                        total,
                    });
                }
            }
            other => extra(other),
        };
        run(&mut sink)?;
    }
    match failure {
        Some(detail) => Err(detail),
        None => Ok(collected),
    }
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
    for set in protocol.sets() {
        if matches!(set.source, SourceSpec::Localizations { .. }) {
            artifacts.push(format!("{}-superres.ome.tif", set.key()));
        }
    }
    if protocol.composition().is_some() {
        artifacts.push(grana_protocol::CompositionSpec::FILE.to_string());
    }
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
            artifacts.push(format!("{stem}.svg"));
        }
    }
    if let Some(coloc) = protocol.coloc() {
        for pair in &coloc.pairs {
            let stem = format!("coloc-{}-{}", slug(&pair.a), slug(&pair.b));
            artifacts.push(format!("{stem}.csv"));
            artifacts.push(format!("{stem}.svg"));
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
        artifacts.push(format!("{}.grana", slug(name)));
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
            camera: view.camera(),
            timeline: view.timeline().unwrap_or_else(fitted),
            export: view.export().unwrap_or(Export::NATIVE),
        },
        None => {
            let mut camera = Camera::new(35.0);
            camera.fit(scene.dataset.world_size(), 1.6);
            ViewState {
                camera: camera.state(),
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
    drifts: grana_scene::Drifts,
}

fn detect(
    task: &Task<'_>,
    reading: Reading<'_>,
    params: ObjectParams,
    seeds: Vec<Vec<[f32; 3]>>,
    policy: grana_analysis::SeedPolicy,
    store: &Arc<SpillStore>,
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
        | RunEvent::DriftSummary { .. }
        | RunEvent::Registration(_)
        | RunEvent::Fiducial { .. }
        | RunEvent::Calibration { .. }
        | RunEvent::Localization { .. }
        | RunEvent::LocalizationReport { .. }
        | RunEvent::LocalizationClusters { .. }
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
            reading.drifts.clone(),
            task.workers,
            Arc::clone(store),
        )
        .with_seeds(seeds, policy),
        progress.cancel,
        &mut sink,
    )?;
    Ok(collected)
}

fn grow_frames(
    reading: Reading<'_>,
    params: grana_object::CellParams,
    sources: Vec<Option<Arc<ObjectFrame>>>,
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
        | RunEvent::DriftSummary { .. }
        | RunEvent::Registration(_)
        | RunEvent::Fiducial { .. }
        | RunEvent::Calibration { .. }
        | RunEvent::Localization { .. }
        | RunEvent::LocalizationReport { .. }
        | RunEvent::LocalizationClusters { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::grow(
        reading.reader,
        run::GrowRequest::new(
            reading.channel,
            params,
            reading.region,
            reading.drifts.clone(),
            reading.roi,
            sources,
        ),
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
        | RunEvent::DriftSummary { .. }
        | RunEvent::Registration(_)
        | RunEvent::Fiducial { .. }
        | RunEvent::Calibration { .. }
        | RunEvent::Localization { .. }
        | RunEvent::LocalizationReport { .. }
        | RunEvent::LocalizationClusters { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::segment(
        reading.reader,
        run::SegmentRequest::new(
            reading.channel,
            params,
            reading.region,
            reading.drifts.clone(),
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

pub(super) fn language() -> grana_i18n::Language {
    grana_i18n::Language::from_environment().unwrap_or_else(grana_i18n::Language::system)
}

fn colocalization(
    result: &grana_scene::ColocResult,
    x: &str,
    y: &str,
    tints: [[f32; 3]; 2],
    name: &str,
    language: grana_i18n::Language,
) -> grana_chart::Page {
    let series = result.series();
    let thresholds = result.thresholds();
    let statistics = result.statistics();
    let manders = statistics.manders();
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
    let heading = grana_chart::Heading::new(
        grana_i18n::coloc::joint_histogram(language),
        format!("Y: {y}"),
        format!("X: {x}"),
    )
    .with_summary(format!("{name} · {caption}"))
    .with_language(language);
    grana_chart::Page::Joint(Box::new(grana_chart::Joint::total(
        heading,
        series,
        tints,
        [thresholds.a(), thresholds.b()],
    )))
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "\u{2014}".to_string(), |value| format!("{value:.3}"))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn stream(path: &Path, table: &grana_measure::Table, rows: &[usize]) -> Result<(), String> {
    let fault = |error: std::io::Error| format!("failed to write {}: {error}", path.display());
    let mut writer = std::io::BufWriter::new(std::fs::File::create(path).map_err(fault)?);
    table
        .write_csv_rows(&mut writer, rows, &mut |_, _| {})
        .map_err(fault)?;
    std::io::Write::flush(&mut writer).map_err(fault)
}
