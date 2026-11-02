use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_image::{Image, Source};
use grana_measure::{Aggregate, Frame, Group, Objects, Table};
use grana_measure::{ColocPair, ColocSeries};
use grana_object::TrackSet;
use grana_scene::ObjectFrame;
use grana_volume::{Pyramid, Region};
use thiserror::Error;

use crate::event::Event as RunEvent;
use crate::protocol::Protocol;
use crate::run;
use crate::slug::slug;

pub struct Plan {
    pub protocol: Protocol,
    pub datasets: Vec<PathBuf>,
    pub output: PathBuf,
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
    Complete { objects: usize, detail: String },
    Failed { detail: String },
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

fn execute(plan: Plan, events: Sender<Event>, cancel: Arc<AtomicBool>) {
    if let Err(detail) = std::fs::create_dir_all(&plan.output) {
        let _ = events.send(Event::Failed(format!(
            "failed to create {}: {detail}",
            plan.output.display()
        )));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    if let Err(detail) = plan.protocol.write(&plan.output.join("protocol.json")) {
        let _ = events.send(Event::Failed(detail.to_string()));
        let _ = events.send(Event::Done { cancelled: false });
        return;
    }
    let mut folders = Vec::new();
    for dataset in &plan.datasets {
        let folder = slug(&dataset_name(dataset)).to_ascii_lowercase();
        if folders.contains(&folder) {
            let _ = events.send(Event::Failed(format!(
                "two datasets share the output folder {folder}"
            )));
            let _ = events.send(Event::Done { cancelled: false });
            return;
        }
        folders.push(folder);
    }
    let mut summary = vec!["dataset,status,objects,detail".to_string()];
    let mut cancelled = false;
    for (index, dataset) in plan.datasets.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let name = dataset_name(dataset);
        let _ = events.send(Event::Started {
            index,
            name: name.clone(),
        });
        match analyse(
            &plan.protocol,
            dataset,
            &plan.output,
            index,
            &events,
            &cancel,
        ) {
            Ok(report) => {
                summary.push(row(&name, "ok", report.objects, &report.detail));
                let _ = events.send(Event::Dataset {
                    index,
                    outcome: Outcome::Complete {
                        objects: report.objects,
                        detail: report.detail,
                    },
                });
            }
            Err(detail) => {
                let _ = std::fs::remove_dir_all(plan.output.join(slug(&name)));
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                summary.push(row(&name, "failed", 0, &detail));
                let _ = events.send(Event::Dataset {
                    index,
                    outcome: Outcome::Failed {
                        detail: detail.clone(),
                    },
                });
            }
        }
    }
    if let Err(error) = std::fs::write(plan.output.join("summary.csv"), summary.join("\n") + "\n") {
        let _ = events.send(Event::Failed(format!(
            "failed to write the batch summary: {error}"
        )));
    }
    let _ = events.send(Event::Done { cancelled });
}

struct Report {
    objects: usize,
    detail: String,
}

fn analyse(
    protocol: &Protocol,
    dataset: &Path,
    output: &Path,
    index: usize,
    events: &Sender<Event>,
    cancel: &AtomicBool,
) -> Result<Report, String> {
    let source = Source::file(dataset.to_path_buf());
    let reader = Image::open(&source)?;
    let pyramid = reader.pyramid();
    let channels = pyramid
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let folder = output.join(slug(&dataset_name(dataset)));
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    let mut objects = 0usize;
    let mut detail = Vec::new();
    for set in protocol.sets() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let channel = resolve(pyramid, &set.channel)
            .map_err(|detail| format!("the set {} cannot start: {detail}", set.name))?;
        let _ = events.send(Event::Step {
            index,
            label: format!("detecting {}", set.name),
        });
        let frames = detect(&reader, channel, set, index, events, cancel)?;
        let detected = frame_refs(&frames);
        let tracks = if let Some(params) = set.track_params() {
            let _ = events.send(Event::Step {
                index,
                label: format!("tracking {}", set.name),
            });
            let Some(tracks) = track(&frames, params, pyramid.frame_interval(), cancel) else {
                return Err("cancelled".to_string());
            };
            Some(tracks)
        } else {
            None
        };
        let table = Table::build(
            &detected,
            tracks.as_ref(),
            &channels,
            pyramid.frame_interval(),
            Group::Object,
            Aggregate::Mean,
        );
        let rows = (0..table.rows()).collect::<Vec<_>>();
        write(
            &folder.join(format!("{}-objects.csv", slug(&set.name))),
            &table.csv(&rows),
        )?;
        objects += table.rows();
        detail.push(format!("{}={}", set.name, table.rows()));
        if let Some(tracks) = &tracks {
            let table = Table::build(
                &detected,
                Some(tracks),
                &channels,
                pyramid.frame_interval(),
                Group::Track,
                Aggregate::Mean,
            );
            let rows = (0..table.rows()).collect::<Vec<_>>();
            write(
                &folder.join(format!("{}-tracks.csv", slug(&set.name))),
                &table.csv(&rows),
            )?;
        }
    }
    if let Some(coloc) = protocol.coloc() {
        let _ = events.send(Event::Step {
            index,
            label: format!("colocalizing {} with {}", coloc.a, coloc.b),
        });
        let pair = ColocPair::new(
            resolve(pyramid, &coloc.a).map_err(|detail| format!("colocalization: {detail}"))?,
            resolve(pyramid, &coloc.b).map_err(|detail| format!("colocalization: {detail}"))?,
        );
        let series = colocalize(&reader, pair, cancel)?.ok_or_else(|| "cancelled".to_string())?;
        let thresholds =
            grana_measure::ColocThresholds::new(coloc.thresholds[0], coloc.thresholds[1]);
        write(
            &folder.join(format!("coloc-{}-{}.csv", slug(&coloc.a), slug(&coloc.b))),
            &series.csv(thresholds),
        )?;
    }
    Ok(Report {
        objects,
        detail: detail.join("; "),
    })
}

fn detect(
    reader: &Image,
    channel: usize,
    set: &crate::protocol::SetSpec,
    index: usize,
    events: &Sender<Event>,
    cancel: &AtomicBool,
) -> Result<Vec<Option<ObjectFrame>>, String> {
    let frames = reader.pyramid().frame_count() as usize;
    let mut collected = vec![None; frames];
    let mut sink = |event: RunEvent| match event {
        RunEvent::Frame { time, frame } => collected[time as usize] = Some(frame),
        RunEvent::Progress { done, total, .. } => {
            let _ = events.send(Event::Progress { index, done, total });
        }
        RunEvent::Tracks(_) | RunEvent::Coloc { .. } | RunEvent::Done | RunEvent::Failed(_) => {}
    };
    run::detect(
        reader,
        channel,
        set.params(),
        Region::none(),
        cancel,
        &mut sink,
    )?;
    Ok(collected)
}

fn track(
    frames: &[Option<ObjectFrame>],
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
    cancel: &AtomicBool,
) -> Result<Option<ColocSeries>, String> {
    let mut series = None;
    let mut sink = |event: RunEvent| {
        if let RunEvent::Coloc { series: found, .. } = event {
            series = Some(*found);
        }
    };
    run::coloc(reader, pair, Region::none(), cancel, &mut sink)?;
    Ok(series)
}

fn frame_refs(frames: &[Option<ObjectFrame>]) -> Vec<Frame<'_>> {
    frames
        .iter()
        .enumerate()
        .filter_map(|(time, frame)| {
            frame.as_ref().map(|frame| {
                Frame::new(
                    time as u32,
                    match frame {
                        ObjectFrame::Spots(set) => Objects::Spots(set),
                        ObjectFrame::Surfaces(set) => Objects::Surfaces(set),
                        ObjectFrame::Filaments(set) => Objects::Filaments(set),
                    },
                )
            })
        })
        .collect()
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

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub fn dataset_name(path: &Path) -> String {
    grana_image::dataset_name(path)
}

fn row(dataset: &str, status: &str, objects: usize, detail: &str) -> String {
    format!("{},{status},{objects},{}", escape(dataset), escape(detail))
}

fn escape(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
