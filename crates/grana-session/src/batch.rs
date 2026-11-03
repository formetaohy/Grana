use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, MutexGuard};

use grana_analysis::Roi;
use grana_animation::Timeline;
use grana_display::{Camera, SectionCamera};
use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::{Format, Image, Source};
use grana_measure::{ColocPair, ColocSeries, Lasso};
use grana_project::{Export, Project, Source as ProjectSource, ViewState};
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectSetId, Scene, slug};
use grana_volume::{Axis, Pyramid, Region};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::Event as RunEvent;
use crate::protocol::{Protocol, ProtocolError, Step};
use crate::run;

const RESULT: &str = "result.json";
const PARTIAL: &str = "partial";
pub const MAX_JOBS: usize = 16;
const DEFAULT_JOBS: usize = 4;
const ANALYSIS_REVISION: u32 = 4;
const CONTENT_SAMPLE: u64 = 1 << 16;
const HASH: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map_or(DEFAULT_JOBS, |cores| cores.get())
        .min(DEFAULT_JOBS)
}

fn dataset_workers(jobs: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, |cores| cores.get())
        .div_ceil(jobs.max(1))
        .max(1)
}

pub struct Plan {
    pub protocol: Protocol,
    pub datasets: Vec<PathBuf>,
    pub output: PathBuf,
    pub jobs: usize,
    pub project: bool,
}

pub enum Event {
    Started {
        index: usize,
        name: String,
    },
    Step {
        index: usize,
        label: String,
    },
    Progress {
        index: usize,
        done: usize,
        total: usize,
    },
    Dataset {
        index: usize,
        outcome: Outcome,
    },
    Failed(String),
    Done {
        cancelled: bool,
    },
}

pub enum Outcome {
    Complete {
        objects: usize,
        detail: String,
        resumed: bool,
    },
    Failed {
        detail: String,
    },
}

pub enum BatchPoll {
    Event(Event),
    Empty,
    Closed,
}

#[derive(Debug, Error)]
pub enum DiscoverError {
    #[error("failed to read {path}: {detail}")]
    Read { path: String, detail: String },
}

pub struct Batch {
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Batch {
    pub fn start(plan: Plan) -> Self {
        let (events, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancel);
        std::thread::spawn(move || execute(plan, events, token));
        Self {
            events: receiver,
            cancel,
        }
    }

    pub fn poll(&self) -> BatchPoll {
        match self.events.try_recv() {
            Ok(event) => BatchPoll::Event(event),
            Err(TryRecvError::Empty) => BatchPoll::Empty,
            Err(TryRecvError::Disconnected) => BatchPoll::Closed,
        }
    }

    pub fn wait(&self) -> Option<Event> {
        self.events.recv().ok()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn discover(root: &Path) -> Result<Vec<PathBuf>, DiscoverError> {
    let fail = |error: std::io::Error| DiscoverError::Read {
        path: root.display().to_string(),
        detail: error.to_string(),
    };
    if grana_image::is_image(root) {
        return Ok(vec![root.to_path_buf()]);
    }
    let mut datasets = std::fs::read_dir(root)
        .map_err(fail)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| grana_image::is_image(path))
        .collect::<Vec<_>>();
    datasets.sort();
    Ok(datasets)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    protocol: u64,
    build: String,
    content: u64,
    artifacts: Vec<String>,
    objects: usize,
    detail: String,
    quality: Vec<Quality>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Quality {
    name: String,
    kind: String,
    objects: usize,
    volume: Option<f64>,
    area: Option<f64>,
    size: Option<f64>,
    intensity: Option<f64>,
}

struct Row {
    status: &'static str,
    objects: usize,
    detail: String,
    quality: Vec<Quality>,
}

struct Report {
    objects: usize,
    detail: String,
    quality: Vec<Quality>,
}

struct Task<'a> {
    protocol: &'a Protocol,
    workers: usize,
    digest: u64,
    index: usize,
    project: bool,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
}

struct Progress<'a> {
    index: usize,
    events: &'a Sender<Event>,
    cancel: &'a AtomicBool,
}

fn execute(plan: Plan, events: Sender<Event>, cancel: Arc<AtomicBool>) {
    if let Err(detail) = prepare(&plan) {
        let _ = events.send(Event::Failed(detail));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    let digest = plan.protocol.digest();
    let next = AtomicUsize::new(0);
    let rows = Mutex::new((0..plan.datasets.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = plan.jobs.clamp(1, MAX_JOBS).min(plan.datasets.len().max(1));
    let workers = dataset_workers(jobs);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let events = events.clone();
            let cancel = Arc::clone(&cancel);
            let next = &next;
            let rows = &rows;
            let plan = &plan;
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= plan.datasets.len() {
                        break;
                    }
                    work(
                        plan,
                        &Task {
                            protocol: &plan.protocol,
                            workers,
                            digest,
                            index,
                            project: plan.project,
                            events: &events,
                            cancel: &cancel,
                        },
                        rows,
                    );
                }
            });
        }
    });
    let mut summary = vec![grana_measure::csv::line(&[
        "dataset", "status", "objects", "detail",
    ])];
    let mut quality = vec![grana_measure::csv::line(&[
        "dataset",
        "status",
        "set",
        "kind",
        "objects",
        "volume_um3",
        "area_um2",
        "size_um",
        "intensity",
    ])];
    for (index, row) in lock(&rows).iter().enumerate() {
        if let Some(row) = row {
            let name = dataset_name(&plan.datasets[index]);
            summary.push(row_line(&name, row.status, row.objects, &row.detail));
            quality.extend(
                row.quality
                    .iter()
                    .map(|entry| quality_line(&name, row.status, entry)),
            );
        }
    }
    if let Err(error) = std::fs::write(plan.output.join("summary.csv"), summary.concat()) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch summary: {error}"
        )));
    }
    if let Err(error) = std::fs::write(plan.output.join("quality.csv"), quality.concat()) {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch quality report: {error}"
        )));
    }
    let _ = events.send(Event::Done {
        cancelled: cancel.load(Ordering::Relaxed),
    });
}

fn prepare(plan: &Plan) -> Result<(), String> {
    std::fs::create_dir_all(&plan.output)
        .map_err(|error| format!("failed to create {}: {error}", plan.output.display()))?;
    plan.protocol
        .write(&plan.output.join("protocol.json"))
        .map_err(|error| error.to_string())?;
    let mut folders = Vec::new();
    for dataset in &plan.datasets {
        let folder = folder_name(dataset);
        if folders.contains(&folder) {
            return Err(format!("two datasets share the output folder {folder}"));
        }
        folders.push(folder);
    }
    Ok(())
}

fn folder_name(dataset: &Path) -> String {
    slug(&dataset_name(dataset)).to_ascii_lowercase()
}

fn work(plan: &Plan, task: &Task<'_>, rows: &Mutex<Vec<Option<Row>>>) {
    let index = task.index;
    let dataset = &plan.datasets[index];
    let name = dataset_name(dataset);
    let folder = plan.output.join(folder_name(dataset));
    let artifacts = artifacts(task.protocol, &name, task.project);
    if let Some(record) = completed(&folder, task.digest, &build(), content(dataset), &artifacts) {
        let _ = task.events.send(Event::Dataset {
            index,
            outcome: Outcome::Complete {
                objects: record.objects,
                detail: record.detail.clone(),
                resumed: true,
            },
        });
        store(
            rows,
            index,
            Row {
                status: "resumed",
                objects: record.objects,
                detail: record.detail,
                quality: record.quality,
            },
        );
        return;
    }
    let _ = task.events.send(Event::Started {
        index,
        name: name.clone(),
    });
    match analyse(task, dataset, &plan.output) {
        Ok(report) => {
            let _ = task.events.send(Event::Dataset {
                index,
                outcome: Outcome::Complete {
                    objects: report.objects,
                    detail: report.detail.clone(),
                    resumed: false,
                },
            });
            store(
                rows,
                index,
                Row {
                    status: "ok",
                    objects: report.objects,
                    detail: report.detail,
                    quality: report.quality,
                },
            );
        }
        Err(detail) => {
            if task.cancel.load(Ordering::Relaxed) {
                return;
            }
            let _ = task.events.send(Event::Dataset {
                index,
                outcome: Outcome::Failed {
                    detail: detail.clone(),
                },
            });
            store(
                rows,
                index,
                Row {
                    status: "failed",
                    objects: 0,
                    detail,
                    quality: Vec::new(),
                },
            );
        }
    }
}

fn analyse(task: &Task<'_>, dataset: &Path, output: &Path) -> Result<Report, String> {
    let folder = output.join(folder_name(dataset));
    let partial = output.join(format!(".{PARTIAL}-{}", folder_name(dataset)));
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
    std::fs::rename(&partial, &folder).map_err(|error| {
        let _ = std::fs::remove_dir_all(&partial);
        format!("failed to write {}: {error}", folder.display())
    })?;
    Ok(report)
}

fn write_dataset(task: &Task<'_>, dataset: &Path, folder: &Path) -> Result<Report, String> {
    let source = Source::file(dataset.to_path_buf());
    let reader = Image::open(&source)?;
    let store = GeometryStore::temporary(DEFAULT_BUDGET)
        .map_err(|error| format!("the dataset cannot page its geometry: {error}"))?;
    let name = dataset_name(dataset);
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
    let Run { scene, sets, .. } = run;
    let mut objects = 0usize;
    let mut detail = Vec::new();
    let mut quality = Vec::new();
    let keys = grana_measure::channel_keys(
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
        quality.push(quality_of(&scene, sets[index], &keys, &table, &rows));
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
                path: dataset.to_path_buf(),
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
        content: content(dataset),
        artifacts,
        objects: report.objects,
        detail: report.detail.clone(),
        quality: report.quality.clone(),
    })
    .map_err(|error| format!("failed to record the dataset: {error}"))?;
    write(&folder.join(RESULT), &record)?;
    Ok(report)
}

fn quality_of(
    scene: &Scene,
    set: ObjectSetId,
    keys: &[String],
    table: &grana_measure::Table,
    rows: &[usize],
) -> Quality {
    let object = scene
        .objects
        .get(set)
        .expect("the batch quality names its object set");
    let mean = |key: &str| -> Option<f64> {
        let column = table.column(key)?;
        if rows.is_empty() {
            return None;
        }
        let total = rows
            .iter()
            .map(|row| f64::from(table.value(*row, column)))
            .sum::<f64>();
        Some(total / rows.len() as f64)
    };
    let intensity = grana_measure::Measurement::Intensity {
        channel: object.channel(),
        statistic: grana_measure::Statistic::Mean,
    }
    .key(keys);
    Quality {
        name: object.name().to_string(),
        kind: object.kind().name().to_string(),
        objects: rows.len(),
        volume: mean("volume_um3"),
        area: mean("area_um2"),
        size: mean("diameter_um").or_else(|| mean("length_um")),
        intensity: mean(&intensity),
    }
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

fn artifacts(protocol: &Protocol, name: &str, project: bool) -> Vec<String> {
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
    set: &crate::protocol::SetSpec,
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

fn completed(
    folder: &Path,
    digest: u64,
    build: &str,
    content: u64,
    artifacts: &[String],
) -> Option<Record> {
    let text = std::fs::read_to_string(folder.join(RESULT)).ok()?;
    let record = serde_json::from_str::<Record>(&text).ok()?;
    (record.protocol == digest
        && record.build == build
        && record.content == content
        && record.artifacts == artifacts
        && artifacts.iter().all(|name| folder.join(name).is_file()))
    .then_some(record)
}

fn build() -> String {
    format!("{}/{}", env!("CARGO_PKG_VERSION"), ANALYSIS_REVISION)
}

fn content(dataset: &Path) -> u64 {
    match std::fs::metadata(dataset) {
        Err(_) => 0,
        Ok(metadata) if metadata.is_dir() => directory_content(dataset),
        Ok(metadata) => file_content(dataset, metadata.len()),
    }
}

fn file_content(path: &Path, length: u64) -> u64 {
    file_bytes(HASH, path, length)
}

fn file_bytes(seed: u64, path: &Path, length: u64) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    let mut hash = mix_u64(seed, length);
    let Ok(mut file) = std::fs::File::open(path) else {
        return hash;
    };
    let span = CONTENT_SAMPLE.min(length);
    let mut buffer = vec![0u8; span as usize];
    for offset in [
        0,
        length.saturating_sub(span) / 2,
        length.saturating_sub(span),
    ] {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            break;
        }
        let Ok(count) = file.read(&mut buffer) else {
            break;
        };
        hash = mix_bytes(hash, &buffer[..count]);
    }
    hash
}

fn directory_content(root: &Path) -> u64 {
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort();
    let mut hash = mix_u64(HASH, entries.len() as u64);
    for (name, size) in entries {
        hash = mix_u64(hash, size);
        hash = mix_u64(hash, name.len() as u64);
        hash = mix_bytes(hash, name.as_bytes());
        hash = file_bytes(hash, &root.join(&name), size);
    }
    hash
}

fn collect(root: &Path, folder: &Path, entries: &mut Vec<(String, u64)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect(root, &path, entries);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((name, metadata.len()));
    }
}

fn mix_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
    }
    hash
}

fn mix_u64(hash: u64, value: u64) -> u64 {
    mix_bytes(hash, &value.to_le_bytes())
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

pub fn dataset_name(path: &Path) -> String {
    grana_image::dataset_name(path)
}

fn row_line(dataset: &str, status: &str, objects: usize, detail: &str) -> String {
    grana_measure::csv::line(&[dataset, status, &objects.to_string(), detail])
}

fn quality_line(dataset: &str, status: &str, quality: &Quality) -> String {
    let formatted = |value: Option<f64>, decimals: usize| match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    };
    grana_measure::csv::line(&[
        dataset,
        status,
        &quality.name,
        &quality.kind,
        &quality.objects.to_string(),
        &formatted(quality.volume, 3),
        &formatted(quality.area, 3),
        &formatted(quality.size, 3),
        &formatted(quality.intensity, 2),
    ])
}

fn lock(rows: &Mutex<Vec<Option<Row>>>) -> MutexGuard<'_, Vec<Option<Row>>> {
    rows.lock().unwrap_or_else(|error| error.into_inner())
}

fn store(rows: &Mutex<Vec<Option<Row>>>, index: usize, row: Row) {
    lock(rows)[index] = Some(row);
}
