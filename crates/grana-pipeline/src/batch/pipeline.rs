use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_animation::Timeline;
use grana_display::{Camera, SectionCamera};
use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::{Format, Image, Source};
use grana_measure::{ColocPair, ColocSeries, Lasso};
use grana_object::Roi;
use grana_project::{Export, Project, Source as ProjectSource, ViewState};
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectSetId, Scene, slug};
use grana_volume::{Axis, Pyramid, Region};

use crate::event::Event as RunEvent;
use crate::run;
use crate::snapshot::SnapshotRequest;
use grana_protocol::{Protocol, ProtocolError, Step};

use super::quality::{self, Quality};
use super::resume::{Record, build, content};
use super::{DatasetEntry, Event, PARTIAL, Progress, RESULT, SNAPSHOT, Task};
pub(super) struct Report {
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
}

pub(super) fn analyse(
    task: &Task<'_>,
    dataset: &DatasetEntry,
    output: &Path,
) -> Result<Report, String> {
    let folder = output.join(dataset.folder());
    let partial = output.join(format!(".{PARTIAL}-{}", dataset.key()));
    clear(&partial)?;
    clear(&folder)?;
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
    std::fs::rename(&partial, &folder).map_err(|error| {
        let _ = std::fs::remove_dir_all(&partial);
        format!("failed to write {}: {error}", folder.display())
    })?;
    Ok(report)
}

fn write_dataset(task: &Task<'_>, dataset: &DatasetEntry, folder: &Path) -> Result<Report, String> {
    let source = Source::file_series(dataset.path().to_path_buf(), dataset.scene());
    let reader = Image::open(&source)?;
    let store = GeometryStore::temporary(DEFAULT_BUDGET)
        .map_err(|error| format!("the dataset cannot page its geometry: {error}"))?;
    let name = dataset.title();
    let artifacts = artifacts(task.protocol, &name, task.project);
    let mut scene = Scene::new(dataset_of(reader.format(), &name, reader.pyramid()));
    let staged = task
        .protocol
        .stage(&mut scene)
        .map_err(|error: ProtocolError| error.to_string())?;
    let mut run = Run {
        task,
        reader: &reader,
        store: &store,
        scene,
        sets: staged.sets().to_vec(),
        pairs: staged.pairs().to_vec(),
        progress: Progress {
            index: task.index,
            events: task.events,
            cancel: task.cancel,
        },
    };
    let mut measure = Measure::new();
    for step in task.protocol.steps() {
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        match step {
            Step::Detect { set, mask } => run.detect(set, mask)?,
            Step::Relation { source, target } => run.relate(source, target)?,
            Step::Coloc { pair, mask } => run.colocalize(pair, mask)?,
            Step::Present => {
                task.protocol
                    .present_measure(&run.sets, &mut measure, &run.scene);
                measure.apply(&mut run.scene);
            }
        }
    }
    let Run {
        mut scene, sets, ..
    } = run;
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
        let file = format!("{}-{}.csv", slug(&spec.name), spec.group().key());
        write(&folder.join(&file), &table.csv(&rows))?;
        objects += rows.len();
        detail.push(format!("{}={}", spec.name, rows.len()));
        quality.push(quality::of(&scene, sets[index], &keys, &table, &rows));
    }
    if task.protocol.coloc().is_some() {
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
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
        }
    }
    let report = Report {
        objects,
        detail: detail.join("; "),
        quality,
    };
    if let Some(spec) = task.protocol.snapshot() {
        let snapshotter = task.snapshot.ok_or_else(|| {
            "the protocol requests quality snapshots but this run carries no renderer".to_string()
        })?;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("rendering the quality snapshot of {}", reader.name()),
        });
        let time = task.protocol.view().map_or(scene.time(), |view| view.time);
        scene.set_time(time.min(scene.frame_count() - 1));
        let request = SnapshotRequest {
            view: task
                .protocol
                .view()
                .map(|_| view_state(task.protocol, &scene)),
            size: spec.size(),
            transparent: spec.transparent,
            time: scene.time(),
        };
        snapshotter.snapshot(&reader, &scene, &request, &folder.join(SNAPSHOT))?;
    }
    if task.project {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("writing the project of {}", reader.name()),
        });
        let project = Project::capture(
            &scene,
            &measure,
            ProjectSource::File {
                name: name.clone(),
                path: dataset.path().to_path_buf(),
                series: dataset.scene(),
            },
            view_state(task.protocol, &scene),
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
        content: content(dataset.path()),
        artifacts,
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
    reader: &'b Image,
    store: &'b Arc<GeometryStore>,
    scene: Scene,
    sets: Vec<ObjectSetId>,
    pairs: Vec<ColocPair>,
    progress: Progress<'a>,
}

impl Run<'_, '_> {
    fn detect(&mut self, set: usize, mask: Option<usize>) -> Result<(), String> {
        let task = self.task;
        let spec = &task.protocol.sets()[set];
        let id = self.sets[set];
        let mask = mask.map(|mask| self.sets[mask]);
        self.scene.set_mask(mask);
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
                    return Err(format!(
                        "the set {} is masked by {name}, which detected no objects",
                        spec.name
                    ));
                }
                self.scene.mask_frames()
            }
        };
        let channel = self
            .scene
            .objects
            .get(id)
            .expect("the staged object set exists")
            .channel();
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("detecting {}", spec.name),
        });
        let frames = detect(
            task,
            spec,
            Reading {
                reader: self.reader,
                channel,
                region: self.scene.region,
                roi,
            },
            self.store,
            &self.progress,
        )?;
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                self.scene.install_shared(id, time as u32, Arc::new(frame));
            }
        }
        self.track(id, &spec.name)
    }

    fn track(&mut self, id: ObjectSetId, name: &str) -> Result<(), String> {
        let Some(object) = self.scene.objects.get(id) else {
            return Ok(());
        };
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

    fn relate(&mut self, source: usize, target: usize) -> Result<(), String> {
        let task = self.task;
        let source_id = self.sets[source];
        let target_id = self.sets[target];
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
        let shift = self
            .scene
            .objects
            .get(source_id)
            .and_then(|set| set.relation(target_id))
            .map_or(0, |relation| relation.shift());
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("relating {source_name} to {target_name}"),
        });
        let progress = &self.progress;
        let scene = &mut self.scene;
        let mut failure = None;
        let mut sink = |event: RunEvent| match event {
            RunEvent::Relation { time, frame } => {
                scene.install_relation(source_id, target_id, time, frame);
            }
            RunEvent::Failed(detail) => failure = Some(detail),
            RunEvent::Progress { done, total, .. } => {
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
            shift,
            task.workers,
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

    fn colocalize(&mut self, pair: usize, mask: Option<usize>) -> Result<(), String> {
        let task = self.task;
        let coloc = task
            .protocol
            .coloc()
            .expect("a colocalization step names its spec");
        let spec = &coloc.pairs[pair];
        let settings = coloc.settings();
        let pair = self.pairs[pair];
        let mask = mask.map(|mask| self.sets[mask]);
        self.scene.set_mask(mask);
        self.scene.coloc.set_settings(settings);
        self.scene.coloc.set_pair(pair);
        let roi = match self.scene.mask {
            None => None,
            Some(mask) => {
                let object = self.scene.objects.get(mask);
                if !object.is_some_and(|set| set.detected() > 0) {
                    let name = object.map_or("the colocalization mask", |set| set.name());
                    return Err(format!("{name} detected no objects"));
                }
                self.scene.mask_frames()
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
            RunEvent::Progress { done, total, .. } => {
                let _ = progress.events.send(Event::Progress {
                    index: progress.index,
                    done,
                    total,
                });
            }
            _ => {}
        };
        run::coloc(
            self.reader,
            run::ColocRequest::new(pair, region, roi, mask_id, settings, task.workers),
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
}

fn frames_of(scene: &Scene, id: ObjectSetId) -> Vec<Option<Arc<ObjectFrame>>> {
    (0..scene.frame_count())
        .map(|time| scene.objects.get(id).and_then(|set| set.frame_shared(time)))
        .collect()
}

pub(super) fn artifacts(protocol: &Protocol, name: &str, project: bool) -> Vec<String> {
    let mut artifacts = protocol
        .sets()
        .iter()
        .map(|set| format!("{}-{}.csv", slug(&set.name), set.group().key()))
        .collect::<Vec<_>>();
    if let Some(coloc) = protocol.coloc() {
        artifacts.extend(
            coloc
                .pairs
                .iter()
                .map(|pair| format!("coloc-{}-{}.csv", slug(&pair.a), slug(&pair.b))),
        );
    }
    if protocol.snapshot().is_some() {
        artifacts.push(SNAPSHOT.to_string());
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
        None => Dataset::synthetic(name, pyramid),
    }
}

fn view_state(protocol: &Protocol, scene: &Scene) -> ViewState {
    let duration = (scene.frame_count() as f32 * scene.dataset.frame_interval())
        .clamp(1.0, Timeline::MAX_DURATION);
    let fitted = || Timeline::new(30, duration);
    match protocol.view() {
        Some(view) => ViewState {
            camera: view.camera(),
            section: view.section(),
            split: view.split,
            timeline: view.timeline().unwrap_or_else(fitted),
            export: view.export().unwrap_or(Export::NATIVE),
        },
        None => {
            let world = scene.dataset.world_size();
            let mut camera = Camera::new(35.0);
            camera.fit(world, 1.6);
            let mut section = SectionCamera::new(Axis::Z);
            section.fit(world, scene.dataset.voxel_size(), 1.6);
            ViewState {
                camera: camera.state(),
                section: section.state(),
                split: 0.62,
                timeline: fitted(),
                export: Export::NATIVE,
            }
        }
    }
}

struct Reading<'a> {
    reader: &'a Image,
    channel: usize,
    region: Region,
    roi: Option<Roi>,
}

fn detect(
    task: &Task<'_>,
    set: &grana_protocol::SetSpec,
    reading: Reading<'_>,
    store: &Arc<GeometryStore>,
    progress: &Progress<'_>,
) -> Result<Vec<Option<ObjectFrame>>, String> {
    let frames = reading.reader.pyramid().frame_count() as usize;
    let mut collected = vec![None; frames];
    let mut sink = |event: RunEvent| match event {
        RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
        RunEvent::Progress { done, total, .. } => {
            let _ = progress.events.send(Event::Progress {
                index: progress.index,
                done,
                total,
            });
        }
        RunEvent::Tracks(_)
        | RunEvent::Relation { .. }
        | RunEvent::Coloc { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::detect(
        reading.reader,
        run::DetectRequest::new(
            reading.channel,
            set.params(),
            reading.region,
            reading.roi,
            set.seeds().to_vec(),
            task.workers,
            Arc::clone(store),
        ),
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

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}
