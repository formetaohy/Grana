use std::sync::Arc;

use grana_image::Image;
use grana_offline::{Event, Handle, Outcome, Plan, Task, Worker};
use grana_pipeline::Channels;
use grana_scene::Scene;

use std::time::Instant;

use grana_display::OutputSize;
use grana_offline::{Destination, Pose};
use grana_session::SessionView;

use super::App;
use crate::bricks::ImageBricks;

pub(crate) enum Report {
    Finished(Outcome),
    Closed,
}

pub(super) struct Export {
    worker: Worker,
    active: Option<Handle>,
    progress: Option<(u32, u32)>,
}

impl Default for Export {
    fn default() -> Self {
        Self::new()
    }
}

impl Export {
    pub fn new() -> Self {
        Self {
            worker: Worker::spawn(),
            active: None,
            progress: None,
        }
    }

    pub fn running(&self) -> bool {
        self.active.is_some()
    }

    pub fn progress(&self) -> Option<(u32, u32)> {
        self.progress
    }

    pub fn start(&mut self, plan: Plan, image: Arc<Image>, scene: &Scene) -> Result<(), String> {
        if self.active.is_some() {
            return Err("an export is already running".to_string());
        }
        let scene = scene.snapshot().into_scene();
        let channels = Channels::render(image, &scene);
        let task = Task {
            plan,
            scene,
            bricks: Arc::new(ImageBricks::new(channels)),
        };
        self.active = Some(self.worker.submit(task));
        self.progress = None;
        Ok(())
    }

    pub fn cancel(&mut self) {
        if let Some(handle) = self.active.as_ref() {
            handle.cancel();
        }
    }

    pub fn poll(&mut self) -> Vec<Report> {
        let (events, alive) = self.worker.drain();
        let mut reports = Vec::new();
        for event in events {
            match event {
                Event::Progress { id, frame, total } => {
                    if self.active.as_ref().is_some_and(|handle| handle.id() == id) {
                        self.progress = Some((frame, total));
                    }
                }
                Event::Finished { id, outcome } => {
                    if self.active.as_ref().is_some_and(|handle| handle.id() == id) {
                        self.active = None;
                        self.progress = None;
                        reports.push(Report::Finished(outcome));
                    }
                }
            }
        }
        if !alive && self.active.take().is_some() {
            self.progress = None;
            reports.push(Report::Closed);
        }
        reports
    }
}

impl App {
    pub(super) fn poll_export(&mut self) {
        for report in self.export.poll() {
            self.output = Some(match report {
                Report::Finished(Outcome::Complete { path }) => {
                    format!("saved {}", grana_offline::name(&path))
                }
                Report::Finished(Outcome::Failed { detail }) => detail,
                Report::Finished(Outcome::Cancelled) => "the export was cancelled".to_string(),
                Report::Closed => "the export worker stopped".to_string(),
            });
        }
    }

    pub(super) fn output_size(&self, width: u32, even: bool) -> grana_display::OutputSize {
        let viewport = self.viewport();
        let height = (f64::from(width) * f64::from(viewport.region[3])
            / f64::from(viewport.region[2]).max(1.0))
        .round()
        .max(1.0) as u32;
        let size = grana_display::OutputSize::of(width, height);
        if even { size.even() } else { size }
    }

    pub(super) fn native_width(&self) -> u32 {
        self.viewport().size[0].max(1)
    }

    pub(super) fn pose(&self) -> Pose {
        Pose {
            time: self.session.scene().map_or(0, |scene| scene.time()),
            camera: self.camera.state(),
        }
    }

    pub(super) fn export_ready(&mut self) -> bool {
        if self.export.running() {
            self.set_output_message("wait for the running export to finish");
            return false;
        }
        if self.session.loading().is_some() {
            self.set_output_message("the dataset is still opening");
            return false;
        }
        if self.session.analysing() {
            self.set_output_message("wait for the running analysis before exporting");
            return false;
        }
        true
    }

    pub(super) fn start_export(&mut self, plan: Plan) {
        let Some(scene) = self.session.scene() else {
            self.set_output_message("open a dataset before exporting");
            return;
        };
        let Some(image) = self.session.image() else {
            self.set_output_message("the dataset is still opening");
            return;
        };
        match self.export.start(plan, image, scene) {
            Ok(()) => {
                self.output = None;
                self.repaint_at = Some(Instant::now());
            }
            Err(detail) => self.output = Some(detail),
        }
    }

    pub(super) fn begin_snapshot(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let export = self.session.export();
        let slug = grana_session::slug(scene.dataset.name());
        let width = if export.image == 0 {
            self.native_width()
        } else {
            export.image
        };
        let suggested = format!("{slug}-image.png");
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save an image")
            .set_file_name(suggested)
            .add_filter("PNG (8-bit)", &["png"])
            .add_filter("OME-TIFF (16-bit)", &["tif", "tiff"])
            .save_file()
        else {
            return;
        };
        let size = self.output_size(width, false);
        let poses = vec![self.pose()];
        self.start_export(Plan {
            destination: Destination::Image { path },
            size,
            transparent: export.transparent,
            poses,
            fps: 0,
            region: scene.region,
        });
    }

    pub(super) fn begin_figure(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let export = self.session.export();
        let slug = grana_session::slug(scene.dataset.name());
        let width = if export.image == 0 {
            self.native_width()
        } else {
            export.image
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a vector figure")
            .set_file_name(format!("{slug}-figure.svg"))
            .add_filter("SVG (vector)", &["svg"])
            .add_filter("PDF (vector)", &["pdf"])
            .save_file()
        else {
            return;
        };
        let size = self.output_size(width, false);
        self.start_export(Plan {
            destination: Destination::Figure { path },
            size,
            transparent: export.transparent,
            poses: vec![self.pose()],
            fps: 0,
            region: scene.region,
        });
    }

    pub(super) fn begin_chart(&mut self) {
        if !self.export_ready() {
            return;
        }
        let prepared = {
            let exporting = self.export.progress();
            let export_message = self.output.clone();
            let session = &self.session;
            let view = SessionView {
                scene: session.scene(),
                measure: session.measure(),
                measuring: session.measuring(),
                timeline: session.timeline(),
                jobs: session.progress(),
                io: session.io(exporting, export_message),
                batch: session.batch(),
                history: session.history(),
                series: session.series(),
                plate: session.plate(),
                labels: session.label_images(),
                views: session.views(),
                recent: session.recent(),
                import: session.import(),
                recovery: session.recovery(),
                export: session.export(),
            };
            self.session.scene().map(|scene| {
                (
                    grana_editor::chart_page(&view, self.workspace.distribution_style),
                    scene.region,
                    grana_session::slug(scene.dataset.name()),
                )
            })
        };
        let Some((chart, region, slug)) = prepared else {
            self.set_output_message("open a dataset before exporting a statistics chart");
            return;
        };
        let Some(chart) = chart else {
            self.set_output_message("choose a statistics plot before exporting its chart");
            return;
        };
        let stem = self.session.measure().view().name();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a statistics chart")
            .set_file_name(format!("{slug}-{stem}.svg"))
            .add_filter("SVG (vector)", &["svg"])
            .add_filter("PDF (vector)", &["pdf"])
            .save_file()
        else {
            return;
        };
        let size = chart.size();
        self.start_export(Plan {
            destination: Destination::Chart {
                path,
                chart: Box::new(chart),
            },
            size: grana_display::OutputSize::of(size[0], size[1]),
            transparent: false,
            poses: vec![self.pose()],
            fps: 0,
            region,
        });
    }

    pub(super) fn begin_lineage(&mut self) {
        if !self.export_ready() {
            return;
        }
        let prepared = {
            let Some(scene) = self.session.scene() else {
                return;
            };
            self.workspace
                .track_set
                .or_else(|| scene.selection().map(|selection| selection.set))
                .and_then(|set| scene.objects.get(set))
                .filter(|set| set.tracks().is_some())
                .or_else(|| scene.objects.iter().find(|set| set.tracks().is_some()))
                .map(|set| {
                    let tracks = set.tracks().expect("the tracked set carries its lineage");
                    (
                        set.id(),
                        tracks.set().len(),
                        grana_session::slug(scene.dataset.name()),
                        scene.region,
                    )
                })
        };
        let Some((set, tracks, slug, region)) = prepared else {
            self.set_output_message("open a tracked object set before exporting its lineage");
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a track lineage")
            .set_file_name(format!("{slug}-lineage.svg"))
            .add_filter("SVG (vector)", &["svg"])
            .add_filter("PDF (vector)", &["pdf"])
            .save_file()
        else {
            return;
        };
        let size =
            match grana_figure::lineage::page_size(tracks, grana_figure::lineage::DEFAULT_WIDTH) {
                Ok(size) => size,
                Err(detail) => {
                    self.set_output_message(&detail);
                    return;
                }
            };
        self.start_export(Plan {
            destination: Destination::Lineage { path, set },
            size: OutputSize::of(size[0], size[1]),
            transparent: false,
            poses: vec![self.pose()],
            fps: 0,
            region,
        });
    }

    pub(super) fn begin_movie(&mut self, transparent: bool) {
        if !self.export_ready() {
            return;
        }
        if self.session.timeline().is_empty() {
            self.set_output_message("set a keyframe before exporting");
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let width = self.session.export().movie;
        let slug = grana_session::slug(scene.dataset.name());
        let (suggested, title, filter, extensions) = if transparent {
            (
                format!("{slug}-movie.mov"),
                "Save a transparent movie",
                "MOV",
                &["mov"][..],
            )
        } else {
            (
                format!("{slug}-movie.mp4"),
                "Save a movie",
                "MP4",
                &["mp4"][..],
            )
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(suggested)
            .add_filter(filter, extensions)
            .save_file()
        else {
            return;
        };
        let poses = grana_offline::poses(self.session.timeline());
        let fps = self.session.timeline().fps();
        let size = self.output_size(width, true);
        self.start_export(Plan {
            destination: Destination::Movie { path },
            size,
            transparent,
            poses,
            fps,
            region: scene.region,
        });
    }

    pub(super) fn begin_sequence(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let export = self.session.export();
        let frames = scene.frame_count();
        if frames == 0 {
            self.set_output_message("the dataset has no time point to export");
            return;
        }
        let stem = grana_session::slug(scene.dataset.name());
        let camera = self.camera.state();
        let Some(parent) = rfd::FileDialog::new()
            .set_title("Choose a folder for the frame sequence")
            .pick_folder()
        else {
            return;
        };
        let width = if export.image == 0 {
            self.native_width()
        } else {
            export.image
        };
        let directory = parent.join(format!("{stem}-sequence"));
        let poses = (0..frames).map(|time| Pose { time, camera }).collect();
        let size = self.output_size(width, false);
        self.start_export(Plan {
            destination: Destination::Sequence { directory, stem },
            size,
            transparent: export.transparent,
            poses,
            fps: 0,
            region: scene.region,
        });
    }

    pub(super) fn cancel_export(&mut self) {
        self.export.cancel();
        self.repaint_at = Some(Instant::now());
    }

    pub(super) fn set_output_message(&mut self, message: &str) {
        self.output = Some(message.to_string());
    }

    pub(super) fn confirm_export_close(&self) {
        rfd::MessageDialog::new()
            .set_title("Export running")
            .set_description("An export is still writing its file. Cancel it before closing grana.")
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}
