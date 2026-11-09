use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_pipeline::label::Labeling;
use grana_scene::ObjectFrame;
use grana_scene::ObjectSetId;
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
    set: ObjectSetId,
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
    fn start(request: Request, set: ObjectSetId) -> Self {
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
            let mut labeling = Labeling::new(shapes, visible, labels, levels, title);
            let properties = labeling.properties();
            let mut fill =
                |level: usize, time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("the label export was cancelled".to_string());
                    }
                    labeling.fill(level, time, origin, extent, data)
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
            set,
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
        match self.begin_labels(set) {
            Ok(()) => {}
            Err(detail) => self.message = Some(crate::Message::error(detail)),
        }
    }

    pub(super) fn export_all_labels(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if self.labels.is_some() || !self.labels_queue.is_empty() {
            self.message = Some(crate::Message::error("a label export is already running"));
            return;
        }
        if self.recipe.is_some() {
            self.message = Some(crate::Message::error(
                "a protocol replay owns the label images it exports",
            ));
            return;
        }
        if scene.dataset.origin() != grana_scene::Origin::Zarr {
            self.message = Some(crate::Message::error(
                "label images are written into local OME-Zarr datasets",
            ));
            return;
        }
        let sets = scene
            .objects
            .iter()
            .filter(|set| set.detected() > 0)
            .map(|set| set.id())
            .collect::<Vec<_>>();
        if sets.is_empty() {
            self.message = Some(crate::Message::error("the scene holds no objects to label"));
            return;
        }
        self.labels_queue = sets;
        self.start_next_label();
    }

    pub(super) fn start_next_label(&mut self) {
        while self.labels.is_none() {
            let Some(set) = self.labels_queue.first().copied() else {
                return;
            };
            self.labels_queue.remove(0);
            if let Err(detail) = self.begin_labels(set) {
                self.message = Some(crate::Message::error(detail));
                continue;
            }
            return;
        }
    }

    pub(super) fn begin_labels(&mut self, set: ObjectSetId) -> Result<(), String> {
        if self.labels.is_some() {
            return Err("a label export is already running".to_string());
        }
        if self
            .analysis
            .iter()
            .any(|analysis| matches!(analysis.job(), grana_scene::Job::Detect(id) | grana_scene::Job::Recut(id) if id == set))
        {
            return Err("the object set is being detected before it is labelled".to_string());
        }
        let Some(scene) = self.scene.as_ref() else {
            return Err("no dataset is open".to_string());
        };
        let Some(object) = scene.objects.get(set) else {
            return Err("the object set left the scene".to_string());
        };
        if scene.dataset.origin() != grana_scene::Origin::Zarr {
            return Err("label images are written into local OME-Zarr datasets".to_string());
        }
        let Some(path) = self
            .source
            .as_ref()
            .and_then(|loader| loader.source().path().map(PathBuf::from))
        else {
            return Err("the dataset has no OME-Zarr folder to label".to_string());
        };
        let frames = (0..scene.frame_count())
            .filter(|time| object.frame(*time).is_some())
            .collect::<Vec<_>>();
        let labels = frames
            .iter()
            .filter_map(|time| object.frame(*time).map(ObjectFrame::object_count))
            .max()
            .unwrap_or(0);
        if labels == 0 {
            return Err(format!("{} holds no objects to label", object.name()));
        }
        if labels > LABEL_LIMIT {
            return Err(format!(
                "a uint16 label image addresses at most {LABEL_LIMIT} objects"
            ));
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
            return Err(format!("{} hides every object it labels", object.name()));
        }
        let levels = scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()].to_vec();
        if levels.is_empty() {
            return Err("the dataset holds no stored resolution level".to_string());
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
        self.labels = Some(Labels::start(request, set));
        Ok(())
    }

    pub(super) fn poll_labels(&mut self) -> bool {
        let Some(mut labels) = self.labels.take() else {
            return false;
        };
        let set = labels.set;
        let mut repaint = false;
        let mut keep = true;
        let mut outcome = None;
        loop {
            match labels.poll() {
                Poll::Event(Event::Progress(_, _)) => repaint = true,
                Poll::Event(Event::Done(result)) => {
                    keep = false;
                    repaint = true;
                    outcome = Some(result);
                    break;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    keep = false;
                    repaint = true;
                    outcome = Some(Err(
                        "the label export stopped before the image was written".to_string()
                    ));
                    break;
                }
            }
        }
        if keep {
            self.labels = Some(labels);
            return repaint;
        }
        let mut success = false;
        self.message = Some(match outcome {
            Some(Ok(written)) => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_label_export(
                        set,
                        Some(grana_scene::LabelExport::new(
                            written.path().display().to_string(),
                            written.frames() as u32,
                            written.labels() as u32,
                        )),
                    );
                }
                success = true;
                crate::Message::info(format!(
                    "wrote {} labels over {} frames to {}",
                    written.labels(),
                    written.frames(),
                    written.path().display()
                ))
            }
            Some(Err(detail)) => crate::Message::error(detail),
            None => crate::Message::error("the label export finished without a result"),
        });
        let recipe = self
            .recipe
            .as_ref()
            .is_some_and(|recipe| matches!(recipe.step(), grana_protocol::Step::Labels { .. }));
        if recipe {
            if success {
                self.advance_step();
                self.drive_recipe();
            } else {
                self.abort_recipe();
            }
        } else {
            self.start_next_label();
        }
        repaint
    }
}
