use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_object::{LabelRaster, Shape};
use grana_scene::{ObjectFrame, ObjectSetId};
use grana_volume::Level;
use grana_zarr::Written;

use crate::task::{Poll, Task};

const LABEL_LIMIT: usize = u16::MAX as usize;

pub(super) enum Event {
    Progress(usize, usize),
    Done(Result<Written, String>),
}

pub(super) struct Labels {
    task: Task<Event>,
    label: String,
    progress: (usize, usize),
}

struct Request {
    path: PathBuf,
    group: String,
    name: String,
    frames: Vec<u32>,
    labels: usize,
    color: [f32; 3],
    opacity: f32,
    shapes: Vec<Option<Arc<ObjectFrame>>>,
    visible: Vec<Vec<bool>>,
    levels: Vec<Level>,
    title: String,
}

impl Labels {
    fn start(request: Request) -> Self {
        let Request {
            path,
            group,
            name,
            frames,
            labels,
            color,
            opacity,
            shapes,
            visible,
            levels,
            title,
        } = request;
        let task = Task::spawn(move |cancel, sink| {
            let properties = (1..=labels)
                .map(|label| (label as u32, format!("{title} {label}")))
                .collect::<Vec<_>>();
            let mut cached: Option<(u32, Shape)> = None;
            let mut fill =
                |level: usize, time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("the label export was cancelled".to_string());
                    }
                    if cached.as_ref().map(|(cached, _)| *cached) != Some(time) {
                        let frame = shapes
                            .get(time as usize)
                            .and_then(|frame| frame.as_ref())
                            .ok_or_else(|| {
                                format!("the object set holds no objects at frame {time}")
                            })?;
                        let shape = frame.shape().map_err(|error| error.to_string())?;
                        if shape.objects() > LABEL_LIMIT {
                            return Err(format!(
                                "a uint16 label image addresses at most {LABEL_LIMIT} objects"
                            ));
                        }
                        cached = Some((time, shape));
                    }
                    let shape = &cached.as_ref().expect("the frame shape is cached").1;
                    let level = levels.get(level).copied().ok_or_else(|| {
                        "the label image names a level the dataset does not hold".to_string()
                    })?;
                    let raster = LabelRaster::new(shape, level);
                    let keep = visible.get(time as usize).map(Vec::as_slice);
                    let rendered = raster.render_visible(
                        keep,
                        origin[0]..origin[0] + extent[0],
                        origin[1]..origin[1] + extent[1],
                        origin[2]..origin[2] + extent[2],
                    );
                    data.copy_from_slice(&rendered);
                    Ok(())
                };
            let mut progress = |done: usize, total: usize| sink(Event::Progress(done, total));
            let outcome = grana_zarr::write_group_labels(
                &path,
                &group,
                grana_zarr::LabelSpec {
                    name: &name,
                    frames: &frames,
                    labels,
                    color,
                    opacity,
                    properties: &properties,
                    fill: &mut fill,
                    progress: &mut progress,
                },
            )
            .map_err(|error| error.to_string());
            sink(Event::Done(outcome));
        });
        Self {
            task,
            label: "Exporting labels".to_string(),
            progress: (0, 0),
        }
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.progress
    }

    pub(super) fn poll(&mut self) -> Poll<Event> {
        loop {
            match self.task.poll() {
                Poll::Event(Event::Progress(done, total)) => self.progress = (done, total),
                other => return other,
            }
        }
    }
}

impl super::Session {
    pub(super) fn export_labels(&mut self, set: ObjectSetId) {
        if self.labels.is_some() {
            self.message = Some(crate::Message::error("a label export is already running"));
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| matches!(analysis.job(), grana_scene::Job::Detect(id) | grana_scene::Job::Recut(id) if id == set))
        {
            self.message = Some(crate::Message::error(
                "the object set is being detected before it is labelled",
            ));
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if scene.dataset.origin() != grana_scene::Origin::Zarr {
            self.message = Some(crate::Message::error(
                "label images are written into local OME-Zarr datasets",
            ));
            return;
        }
        let Some(path) = self
            .source
            .as_ref()
            .and_then(|loader| loader.source().path().map(PathBuf::from))
        else {
            self.message = Some(crate::Message::error(
                "the dataset has no OME-Zarr folder to label",
            ));
            return;
        };
        let frames = (0..scene.frame_count())
            .filter(|time| object.frame(*time).is_some())
            .collect::<Vec<_>>();
        if frames.is_empty() {
            self.message = Some(crate::Message::error(format!(
                "{} holds no objects to label",
                object.name()
            )));
            return;
        }
        let labels = frames
            .iter()
            .filter_map(|time| object.frame(*time).map(ObjectFrame::object_count))
            .max()
            .unwrap_or(0);
        if labels == 0 {
            self.message = Some(crate::Message::error(format!(
                "{} holds no objects to label",
                object.name()
            )));
            return;
        }
        if labels > LABEL_LIMIT {
            self.message = Some(crate::Message::error(format!(
                "a uint16 label image addresses at most {LABEL_LIMIT} objects"
            )));
            return;
        }
        let shapes = (0..scene.frame_count())
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        let visible = (0..scene.frame_count())
            .map(|time| {
                object.frame(time).map_or_else(Vec::new, |frame| {
                    (0..frame.object_count())
                        .map(|index| object.visible_object(time, index))
                        .collect()
                })
            })
            .collect::<Vec<_>>();
        if visible.iter().flatten().all(|shown| !shown) {
            self.message = Some(crate::Message::error(format!(
                "{} hides every object it labels",
                object.name()
            )));
            return;
        }
        let levels = scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()].to_vec();
        if levels.is_empty() {
            return;
        }
        let group = self
            .source
            .as_ref()
            .map(|loader| loader.source())
            .and_then(|source| source.group().map(str::to_string))
            .unwrap_or_else(|| "/".to_string());
        let request = Request {
            path,
            group: grana_zarr::group_path(Path::new(&group)),
            name: grana_scene::slug(object.name()),
            frames,
            labels,
            color: object.color(),
            opacity: object.opacity(),
            shapes,
            visible,
            levels,
            title: object.name().to_string(),
        };
        self.message = None;
        self.labels = Some(Labels::start(request));
    }

    pub(super) fn poll_labels(&mut self) -> bool {
        let Some(mut labels) = self.labels.take() else {
            return false;
        };
        let mut repaint = false;
        let mut keep = true;
        loop {
            match labels.poll() {
                Poll::Event(Event::Progress(_, _)) => repaint = true,
                Poll::Event(Event::Done(outcome)) => {
                    keep = false;
                    repaint = true;
                    self.message = Some(match outcome {
                        Ok(written) => crate::Message::info(format!(
                            "wrote {} labels over {} frames to {}",
                            written.labels(),
                            written.frames(),
                            written.path().display()
                        )),
                        Err(detail) => crate::Message::error(detail),
                    });
                    break;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    keep = false;
                    repaint = true;
                    self.message = Some(crate::Message::error(
                        "the label export stopped before the image was written",
                    ));
                    break;
                }
            }
        }
        if keep {
            self.labels = Some(labels);
        }
        repaint
    }
}
