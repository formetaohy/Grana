use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, MutexGuard};

use glam::Vec3;
use grana_analysis::Roi;
use grana_animation::Timeline;
use grana_display::{Camera, SectionCamera};
use grana_geometry::{DEFAULT_BUDGET, MeshStore};
use grana_image::{Format, Image, Source};
use grana_measure::{ColocPair, ColocSeries};
use grana_measure::{Frame, RelationRow, Table};
use grana_object::{RelationFrame, TrackSet};
use grana_project::{Project, Source as ProjectSource, ViewState};
use grana_scene::{Dataset, Measure, ObjectFrame, Scene, slug};
use grana_volume::{Axis, Pyramid, Region};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::Event as RunEvent;
use crate::protocol::{Protocol, ProtocolError};
use crate::run;

const RESULT: &str = "result.json";
const PARTIAL: &str = "partial";
pub const MAX_JOBS: usize = 16;
const DEFAULT_JOBS: usize = 4;
const ANALYSIS_REVISION: u32 = 1;
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
    #[serde(default)]
    project: bool,
    objects: usize,
    detail: String,
}

struct Row {
    status: &'static str,
    objects: usize,
    detail: String,
}

struct Report {
    objects: usize,
    detail: String,
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
    let mut summary = vec!["dataset,status,objects,detail".to_string()];
    for (index, row) in lock(&rows).iter().enumerate() {
        if let Some(row) = row {
            summary.push(row_line(
                &dataset_name(&plan.datasets[index]),
                row.status,
                row.objects,
                &row.detail,
            ));
        }
    }
    if let Err(error) = std::fs::write(plan.output.join("summary.csv"), summary.join("\n") + "\n") {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch summary: {error}"
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
    if let Some(record) = completed(
        &folder,
        task.digest,
        &build(),
        content(dataset),
        task.project,
    ) {
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
    let store = MeshStore::temporary(DEFAULT_BUDGET)
        .map_err(|error| format!("the dataset cannot page its geometry: {error}"))?;
    let pyramid = reader.pyramid();
    let channels = pyramid
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let region = task
        .protocol
        .region()
        .map_or_else(Region::none, |region| region.region(world(pyramid)));
    let mut objects = 0usize;
    let mut detail = Vec::new();
    let progress = Progress {
        index: task.index,
        events: task.events,
        cancel: task.cancel,
    };
    let mut detected = Vec::<Detected>::new();
    for set in task.protocol.sets() {
        if task.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let channel = resolve(pyramid, &set.channel)
            .map_err(|detail| format!("the set {} cannot start: {detail}", set.name))?;
        let roi = match set.mask() {
            Some(mask) => match detected.iter().find(|detected| detected.set.name == *mask) {
                Some(detected) if detected.frames.iter().any(Option::is_some) => {
                    Some(paged_roi(&detected.frames))
                }
                Some(_) => {
                    return Err(format!(
                        "the set {} is masked by {mask}, which detected no objects",
                        set.name
                    ));
                }
                None => {
                    return Err(format!(
                        "the set {} is masked by {mask}, which holds no detections yet",
                        set.name
                    ));
                }
            },
            None => None,
        };
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("detecting {}", set.name),
        });
        let frames = detect(
            task,
            set,
            Reading {
                reader: &reader,
                channel,
                region,
                roi,
            },
            &store,
            &progress,
        )?
        .into_iter()
        .map(|frame| frame.map(Arc::new))
        .collect::<Vec<_>>();
        let tracks = if let Some(params) = set.track_params() {
            let _ = task.events.send(Event::Step {
                index: task.index,
                label: format!("tracking {}", set.name),
            });
            let Some(tracks) = track(&frames, params, pyramid.frame_interval(), task.cancel) else {
                return Err("cancelled".to_string());
            };
            Some(tracks)
        } else {
            None
        };
        detected.push(Detected {
            set,
            frames,
            tracks,
            relations: Vec::new(),
        });
    }
    for relation in task.protocol.relations() {
        let source = detected
            .iter()
            .position(|detected| detected.set.name == relation.source)
            .ok_or_else(|| format!("the relation source {} is unknown", relation.source))?;
        let target = detected
            .iter()
            .position(|detected| detected.set.name == relation.target)
            .ok_or_else(|| format!("the relation target {} is unknown", relation.target))?;
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("relating {} to {}", relation.source, relation.target),
        });
        let frames = relate(
            detected[source].frames.clone(),
            detected[target].frames.clone(),
            task.workers,
            &progress,
        )?;
        detected[source]
            .relations
            .push((relation.target.clone(), frames));
    }
    for detected in &detected {
        let rows = relation_rows(detected);
        let frames = table_frames(&detected.frames, &rows);
        let table = Table::build(
            &frames,
            detected.tracks.as_ref(),
            &channels,
            pyramid.frame_interval(),
            detected.set.group(),
            detected.set.aggregate(),
        );
        let filters = detected.set.filters(&table)?;
        let rows = table.matching(&filters, None, None);
        let file = folder.join(format!(
            "{}-{}.csv",
            slug(&detected.set.name),
            detected.set.group().key()
        ));
        write(&file, &table.csv(&rows))?;
        objects += rows.len();
        detail.push(format!("{}={}", detected.set.name, rows.len()));
    }
    if let Some(coloc) = task.protocol.coloc() {
        let mask_frames = match &coloc.mask {
            None => None,
            Some(name) => match detected.iter().find(|detected| detected.set.name == *name) {
                Some(detected) if detected.frames.iter().any(Option::is_some) => {
                    Some(detected.frames.clone())
                }
                Some(_) => {
                    return Err(format!(
                        "the colocalization mask {name} detected no objects"
                    ));
                }
                None => {
                    return Err(format!("the colocalization mask {name} is unknown"));
                }
            },
        };
        let settings = coloc.settings();
        for spec in &coloc.pairs {
            let _ = task.events.send(Event::Step {
                index: task.index,
                label: format!("colocalizing {} with {}", spec.a, spec.b),
            });
            let pair = ColocPair::new(
                resolve(pyramid, &spec.a).map_err(|detail| format!("colocalization: {detail}"))?,
                resolve(pyramid, &spec.b).map_err(|detail| format!("colocalization: {detail}"))?,
            );
            let roi = mask_frames.as_ref().map(|frames| paged_roi(frames));
            let series = colocalize(
                &reader,
                pair,
                region,
                roi,
                settings,
                task.workers,
                task.cancel,
            )?
            .ok_or_else(|| "cancelled".to_string())?;
            let thresholds =
                grana_measure::ColocThresholds::new(spec.thresholds[0], spec.thresholds[1]);
            write(
                &folder.join(format!("coloc-{}-{}.csv", slug(&spec.a), slug(&spec.b))),
                &series.csv(thresholds),
            )?;
        }
    }
    let report = Report {
        objects,
        detail: detail.join("; "),
    };
    if task.project {
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("writing the project of {}", reader.name()),
        });
        write_project(task, dataset, &reader, detected, folder)?;
    }
    let record = serde_json::to_string(&Record {
        protocol: task.digest,
        build: build(),
        content: content(dataset),
        project: task.project,
        objects: report.objects,
        detail: report.detail.clone(),
    })
    .map_err(|error| format!("failed to record the dataset: {error}"))?;
    write(&folder.join(RESULT), &record)?;
    Ok(report)
}

fn write_project(
    task: &Task<'_>,
    dataset: &Path,
    reader: &Image,
    detected: Vec<Detected<'_>>,
    folder: &Path,
) -> Result<(), String> {
    let name = reader.name().to_string();
    let mut scene = Scene::new(dataset_of(reader.format(), &name, reader.pyramid()));
    let staged = task
        .protocol
        .stage(&mut scene)
        .map_err(|error: ProtocolError| error.to_string())?;
    let order = staged.sets().to_vec();
    let mut pending = Vec::new();
    let position = |name: &str| {
        task.protocol
            .sets()
            .iter()
            .position(|set| set.name == name)
            .expect("a checked protocol names its own object sets")
    };
    for (index, detected) in detected.into_iter().enumerate() {
        let id = order[index];
        let Detected {
            frames,
            tracks,
            relations,
            ..
        } = detected;
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                scene.install_shared(id, time as u32, frame);
            }
        }
        if let Some(tracks) = tracks
            && !tracks.is_empty()
        {
            scene.install_tracks(id, tracks);
        }
        pending.extend(
            relations
                .into_iter()
                .map(|(target, frames)| (id, position(&target), frames)),
        );
    }
    for (id, target, frames) in pending {
        let target = order[target];
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                scene.install_relation(id, target, time as u32, frame);
            }
        }
    }
    let mut measure = Measure::new();
    task.protocol
        .present_measure(staged.sets(), &mut measure, &scene);
    let view = view_state(task.protocol, &scene);
    let project = Project::capture(
        &scene,
        &measure,
        ProjectSource::File {
            name: name.clone(),
            path: dataset.to_path_buf(),
        },
        view,
    );
    let file = folder.join(format!("{}.granaproj", slug(&name)));
    let mut progress = |done: usize, total: usize| {
        let _ = task.events.send(Event::Progress {
            index: task.index,
            done,
            total,
        });
    };
    project
        .write(&file, task.cancel, &mut progress)
        .map_err(|error| format!("failed to write {}: {error}", file.display()))
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
    let timeline = Timeline::new(30, duration);
    if let Some(view) = protocol.view() {
        return ViewState {
            camera: view.camera(),
            section: view.section(),
            split: view.split,
            timeline,
        };
    }
    let world = scene.dataset.world_size();
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.6);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, scene.dataset.voxel_size(), 1.6);
    ViewState {
        camera: camera.state(),
        section: section.state(),
        split: 0.62,
        timeline,
    }
}

struct Detected<'a> {
    set: &'a crate::protocol::SetSpec,
    frames: Vec<Option<Arc<ObjectFrame>>>,
    tracks: Option<TrackSet>,
    relations: Vec<(String, Vec<Option<RelationFrame>>)>,
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
    store: &Arc<MeshStore>,
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

fn track(
    frames: &[Option<Arc<ObjectFrame>>],
    params: grana_analysis::TrackParams,
    frame_interval: f32,
    cancel: &AtomicBool,
) -> Option<TrackSet> {
    let mut detections = vec![Vec::new(); frames.len()];
    for (time, frame) in frames.iter().enumerate() {
        if let Some(frame) = frame {
            detections[time] = frame.centroids();
        }
    }
    let mut tracks = None;
    let mut sink = |event: RunEvent| {
        if let RunEvent::Tracks(set) = event {
            tracks = Some(set);
        }
    };
    run::tracks(detections, params, frame_interval, cancel, &mut sink);
    tracks
}

fn colocalize(
    reader: &Image,
    pair: ColocPair,
    region: Region,
    roi: Option<grana_analysis::Roi>,
    settings: grana_measure::ColocSettings,
    workers: usize,
    cancel: &AtomicBool,
) -> Result<Option<ColocSeries>, String> {
    let mut series = None;
    let mut sink = |event: RunEvent| {
        if let RunEvent::Coloc { series: found, .. } = event {
            series = Some(*found);
        }
    };
    run::coloc(
        reader,
        run::ColocRequest::new(pair, region, roi, None, settings, workers),
        cancel,
        &mut sink,
    )?;
    Ok(series)
}

fn relate(
    source: Vec<Option<Arc<ObjectFrame>>>,
    target: Vec<Option<Arc<ObjectFrame>>>,
    workers: usize,
    progress: &Progress<'_>,
) -> Result<Vec<Option<RelationFrame>>, String> {
    let mut frames = vec![None; source.len()];
    let mut sink = |event: RunEvent| match event {
        RunEvent::Relation { time, frame } => frames[time as usize] = Some(frame),
        RunEvent::Progress { done, total, .. } => {
            let _ = progress.events.send(Event::Progress {
                index: progress.index,
                done,
                total,
            });
        }
        RunEvent::Frame { .. }
        | RunEvent::Tracks(_)
        | RunEvent::Coloc { .. }
        | RunEvent::Done
        | RunEvent::Failed(_) => {}
    };
    run::relation(source, target, workers, progress.cancel, &mut sink);
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    Ok(frames)
}

fn paged_roi(frames: &[Option<Arc<ObjectFrame>>]) -> Roi {
    let frames = Arc::new(frames.to_vec());
    let count = frames.len();
    Roi::paged(count, move |time| shape_at(&frames, time))
}

fn shape_at(
    frames: &[Option<Arc<ObjectFrame>>],
    time: u32,
) -> Result<Option<grana_analysis::Shape>, String> {
    let Some(frame) = frames.get(time as usize).and_then(|frame| frame.as_deref()) else {
        return Ok(None);
    };
    frame.shape().map(Some).map_err(|error| error.to_string())
}

fn relation_rows<'a>(detected: &'a Detected<'a>) -> Vec<Vec<RelationRow<'a>>> {
    (0..detected.frames.len())
        .map(|time| {
            detected
                .relations
                .iter()
                .map(|(target, frames)| RelationRow::new(target, frames[time].as_ref()))
                .collect()
        })
        .collect()
}

fn table_frames<'a>(
    frames: &'a [Option<Arc<ObjectFrame>>],
    relations: &'a [Vec<RelationRow<'a>>],
) -> Vec<Frame<'a>> {
    frames
        .iter()
        .enumerate()
        .filter_map(|(time, frame)| {
            frame
                .as_ref()
                .map(|frame| Frame::new(time as u32, frame.objects(), &relations[time]))
        })
        .collect()
}

fn completed(
    folder: &Path,
    digest: u64,
    build: &str,
    content: u64,
    project: bool,
) -> Option<Record> {
    let text = std::fs::read_to_string(folder.join(RESULT)).ok()?;
    let record = serde_json::from_str::<Record>(&text).ok()?;
    (record.protocol == digest
        && record.build == build
        && record.content == content
        && record.project == project)
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

fn resolve(pyramid: &Pyramid, name: &str) -> Result<usize, String> {
    let matches = pyramid
        .channels()
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.name() == name)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(format!("the dataset has no channel named {name}")),
        [index] => Ok(*index),
        _ => Err(format!("the dataset names more than one channel {name}")),
    }
}

fn world(pyramid: &Pyramid) -> Vec3 {
    Vec3::from(pyramid.level(0).world_extent())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub fn dataset_name(path: &Path) -> String {
    grana_image::dataset_name(path)
}

fn row_line(dataset: &str, status: &str, objects: usize, detail: &str) -> String {
    format!("{},{status},{objects},{}", escape(dataset), escape(detail))
}

fn escape(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn lock(rows: &Mutex<Vec<Option<Row>>>) -> MutexGuard<'_, Vec<Option<Row>>> {
    rows.lock().unwrap_or_else(|error| error.into_inner())
}

fn store(rows: &Mutex<Vec<Option<Row>>>, index: usize, row: Row) {
    lock(rows)[index] = Some(row);
}
