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

    pub fn start(
        &mut self,
        plan: Plan,
        image: Arc<Image>,
        scene: &Scene,
        language: grana_i18n::Language,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err(grana_i18n::app::export_running_error(language).to_string());
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
        let language = self.workspace.language;
        for report in self.export.poll() {
            self.output = Some(match report {
                Report::Finished(Outcome::Complete { path }) => {
                    grana_i18n::app::saved(language, &grana_offline::name(&path))
                }
                Report::Finished(Outcome::Failed { detail }) => detail,
                Report::Finished(Outcome::Cancelled) => {
                    grana_i18n::app::export_cancelled(language).to_string()
                }
                Report::Closed => grana_i18n::app::export_worker_stopped(language).to_string(),
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
        let language = self.workspace.language;
        if self.export.running() {
            self.set_output_message(grana_i18n::app::wait_export(language));
            return false;
        }
        if self.session.loading().is_some() {
            self.set_output_message(grana_i18n::app::dataset_opening(language));
            return false;
        }
        if self.session.analysing() {
            self.set_output_message(grana_i18n::app::wait_analysis(language));
            return false;
        }
        true
    }

    pub(super) fn start_export(&mut self, plan: Plan) {
        let language = self.workspace.language;
        let Some(scene) = self.session.scene() else {
            self.set_output_message(grana_i18n::app::open_dataset_first(language));
            return;
        };
        let Some(image) = self.session.image() else {
            self.set_output_message(grana_i18n::app::dataset_opening(language));
            return;
        };
        match self.export.start(plan, image, scene, language) {
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
        let language = self.workspace.language;
        let Some(path) = rfd::FileDialog::new()
            .set_title(grana_i18n::app::save_image(language))
            .set_file_name(suggested)
            .add_filter(grana_i18n::app::png_8(language), &["png"])
            .add_filter(grana_i18n::app::ome_tiff_16(language), &["tif", "tiff"])
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
            .set_title(grana_i18n::app::save_figure(self.workspace.language))
            .set_file_name(format!("{slug}-figure.svg"))
            .add_filter(
                grana_i18n::app::svg_vector(self.workspace.language),
                &["svg"],
            )
            .add_filter(
                grana_i18n::app::pdf_vector(self.workspace.language),
                &["pdf"],
            )
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
        let stem = self.session.measure().view().name().to_string();
        let language = self.workspace.language;
        self.begin_page(
            grana_i18n::app::save_statistics_chart(language),
            &stem,
            grana_i18n::app::choose_plot(language),
            |view, workspace| {
                grana_editor::chart_page(view, workspace.distribution_style, workspace.language)
            },
        );
    }

    pub(super) fn begin_comparison(&mut self) {
        let language = self.workspace.language;
        self.begin_page(
            grana_i18n::app::save_comparison_chart(language),
            "comparison",
            grana_i18n::app::choose_condition(language),
            grana_editor::comparison_page,
        );
    }

    fn begin_page(
        &mut self,
        title: &str,
        stem: &str,
        missing: &str,
        build: impl FnOnce(
            &SessionView<'_>,
            &grana_editor::Workspace,
        ) -> Option<grana_editor::ChartPage>,
    ) {
        if !self.export_ready() {
            return;
        }
        let prepared = {
            let exporting = self.export.progress();
            let export_message = self.output.clone();
            let session = &self.session;
            let view = SessionView {
                access: session.access(),
                scene: session.scene(),
                measure: session.measure(),
                measuring: session.measuring(),
                timeline: session.timeline(),
                jobs: session.progress(),
                io: session.io(exporting, export_message),
                batch: session.batch(),
                stitch: session.stitch().map(|stitch| stitch.view()),
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
                    build(&view, &self.workspace),
                    scene.region,
                    grana_session::slug(scene.dataset.name()),
                )
            })
        };
        let Some((page, region, slug)) = prepared else {
            self.set_output_message(grana_i18n::app::open_dataset_chart(self.workspace.language));
            return;
        };
        let Some(page) = page else {
            self.set_output_message(missing);
            return;
        };
        let language = self.workspace.language;
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(format!("{slug}-{stem}.svg"))
            .add_filter(grana_i18n::app::svg_vector(language), &["svg"])
            .add_filter(grana_i18n::app::pdf_vector(language), &["pdf"])
            .save_file()
        else {
            return;
        };
        let size = page.size();
        self.start_export(Plan {
            destination: Destination::Chart { path, page },
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
            self.set_output_message(grana_i18n::app::open_tracked_set(self.workspace.language));
            return;
        };
        let language = self.workspace.language;
        let Some(path) = rfd::FileDialog::new()
            .set_title(grana_i18n::app::save_lineage(language))
            .set_file_name(format!("{slug}-lineage.svg"))
            .add_filter(grana_i18n::app::svg_vector(language), &["svg"])
            .add_filter(grana_i18n::app::pdf_vector(language), &["pdf"])
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
            self.set_output_message(grana_i18n::app::set_keyframe(self.workspace.language));
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
                grana_i18n::app::save_transparent_movie(self.workspace.language),
                "MOV",
                &["mov"][..],
            )
        } else {
            (
                format!("{slug}-movie.mp4"),
                grana_i18n::app::save_movie(self.workspace.language),
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
            self.set_output_message(grana_i18n::app::no_time_point(self.workspace.language));
            return;
        }
        let stem = grana_session::slug(scene.dataset.name());
        let camera = self.camera.state();
        let Some(parent) = rfd::FileDialog::new()
            .set_title(grana_i18n::app::choose_sequence_folder(
                self.workspace.language,
            ))
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
        let language = self.workspace.language;
        rfd::MessageDialog::new()
            .set_title(grana_i18n::app::export_running(language))
            .set_description(grana_i18n::app::export_running_detail(language))
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}
