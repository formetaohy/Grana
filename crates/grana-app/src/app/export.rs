use std::sync::Arc;

use grana_exec::Poll;
use grana_image::Image;
use grana_offline::{Event, Handle, Outcome, Plan, Task, Worker};
use grana_pipeline::Channels;
use grana_scene::Scene;

use std::time::Instant;

use grana_display::OutputSize;
use grana_offline::{Destination, Pose};
use grana_session::{Inquiry, SessionView};

use super::App;
use super::editor::session_view;
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
        surfaces: Arc<dyn grana_offline::Surfaces>,
        language: grana_i18n::Language,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err(grana_i18n::app::export_running_error(language).to_string());
        }
        let scene = scene.snapshot().into_scene();
        let channels = Arc::new(Channels::render(image, &scene));
        let task = Task {
            plan,
            scene,
            bricks: Arc::new(ImageBricks::new(channels)),
            surfaces,
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
        let Some(handle) = self.active.as_ref() else {
            return Vec::new();
        };
        let mut reports = Vec::new();
        let mut ended = false;
        loop {
            match handle.poll() {
                Poll::Event(Event::Progress { frame, total }) => {
                    self.progress = Some((frame, total));
                }
                Poll::Event(Event::Finished { outcome }) => {
                    reports.push(Report::Finished(outcome));
                    ended = true;
                    break;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    reports.push(Report::Closed);
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.active = None;
            self.progress = None;
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
        self.viewport().region[2].max(1)
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
        let Some(cache) = self.session.surface_cache() else {
            self.set_output_message(grana_i18n::app::dataset_opening(language));
            return;
        };
        let surfaces = Arc::new(crate::surfaces::Traced::new(cache));
        match self.export.start(plan, image, scene, surfaces, language) {
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
        let language = self.workspace.language;
        self.ask(
            Inquiry::save(
                grana_i18n::app::save_image(language),
                format!("{slug}-image.png"),
            )
            .filter(grana_i18n::app::png_8(language), &["png"])
            .filter(grana_i18n::app::ome_tiff_16(language), &["tif", "tiff"]),
            move |app, path| {
                let Some(path) = path else {
                    return;
                };
                app.export_image(path, width);
            },
        );
    }

    fn export_image(&mut self, path: std::path::PathBuf, width: u32) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let size = self.output_size(width, false);
        let poses = vec![self.pose()];
        self.start_export(Plan {
            destination: Destination::Image { path },
            size,
            transparent: self.session.export().transparent,
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
        let slug = grana_session::slug(scene.dataset.name());
        let width = self.page_width();
        let language = self.workspace.language;
        self.ask(
            Inquiry::save(
                grana_i18n::app::save_figure(language),
                format!("{slug}-figure.svg"),
            )
            .filter(grana_i18n::app::svg_vector(language), &["svg"])
            .filter(grana_i18n::app::pdf_vector(language), &["pdf"]),
            move |app, path| {
                let Some(path) = path else {
                    return;
                };
                app.export_figure(path, width);
            },
        );
    }

    fn export_figure(&mut self, path: std::path::PathBuf, width: u32) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let size = self.output_size(width, false);
        let poses = vec![self.pose()];
        self.start_export(Plan {
            destination: Destination::Figure { path },
            size,
            transparent: self.session.export().transparent,
            poses,
            fps: 0,
            region: scene.region,
        });
    }

    fn page_width(&self) -> u32 {
        let export = self.session.export();
        if export.image == 0 {
            self.native_width()
        } else {
            export.image
        }
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
        ) -> Option<grana_editor::ChartPage>
        + Send
        + 'static,
    ) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            self.set_output_message(grana_i18n::app::open_dataset_chart(self.workspace.language));
            return;
        };
        let slug = grana_session::slug(scene.dataset.name());
        let language = self.workspace.language;
        let missing = missing.to_string();
        self.ask(
            Inquiry::save(title, format!("{slug}-{stem}.svg"))
                .filter(grana_i18n::app::svg_vector(language), &["svg"])
                .filter(grana_i18n::app::pdf_vector(language), &["pdf"]),
            move |app, path| {
                let Some(path) = path else {
                    return;
                };
                if !app.export_page(path, build) {
                    app.set_output_message(&missing);
                }
            },
        );
    }

    fn export_page(
        &mut self,
        path: std::path::PathBuf,
        build: impl FnOnce(
            &SessionView<'_>,
            &grana_editor::Workspace,
        ) -> Option<grana_editor::ChartPage>,
    ) -> bool {
        if !self.export_ready() {
            return true;
        }
        let App {
            session,
            workspace,
            export,
            output,
            ..
        } = self;
        let view = session_view(
            session,
            export.progress(),
            output.as_deref(),
            None,
            grana_session::Unreadable::default(),
        );
        let Some(page) = build(&view, workspace) else {
            return false;
        };
        let Some(region) = session.scene().map(|scene| scene.region) else {
            return false;
        };
        drop(view);
        let size = page.size();
        self.start_export(Plan {
            destination: Destination::Chart { path, page },
            size: grana_display::OutputSize::of(size[0], size[1]),
            transparent: false,
            poses: vec![self.pose()],
            fps: 0,
            region,
        });
        true
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
                    (
                        set.id(),
                        set.tracks().map_or(0, |tracks| tracks.set().len()),
                        grana_session::slug(scene.dataset.name()),
                    )
                })
        };
        let Some((set, tracks, slug)) = prepared else {
            self.set_output_message(grana_i18n::app::open_tracked_set(self.workspace.language));
            return;
        };
        let language = self.workspace.language;
        self.ask(
            Inquiry::save(
                grana_i18n::app::save_lineage(language),
                format!("{slug}-lineage.svg"),
            )
            .filter(grana_i18n::app::svg_vector(language), &["svg"])
            .filter(grana_i18n::app::pdf_vector(language), &["pdf"]),
            move |app, path| {
                let Some(path) = path else {
                    return;
                };
                app.export_lineage(path, set, tracks);
            },
        );
    }

    fn export_lineage(
        &mut self,
        path: std::path::PathBuf,
        set: grana_scene::ObjectSetId,
        tracks: usize,
    ) {
        if !self.export_ready() {
            return;
        }
        let Some(region) = self.session.scene().map(|scene| scene.region) else {
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
        self.ask(
            Inquiry::save(title, suggested).filter(filter, extensions),
            move |app, path| {
                let Some(path) = path else {
                    return;
                };
                app.export_movie(path, transparent, width);
            },
        );
    }

    fn export_movie(&mut self, path: std::path::PathBuf, transparent: bool, width: u32) {
        if !self.export_ready() {
            return;
        }
        let Some(region) = self.session.scene().map(|scene| scene.region) else {
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
            region,
        });
    }

    pub(super) fn begin_sequence(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let frames = scene.frame_count();
        if frames == 0 {
            self.set_output_message(grana_i18n::app::no_time_point(self.workspace.language));
            return;
        }
        let stem = grana_session::slug(scene.dataset.name());
        let width = self.page_width();
        self.ask(
            Inquiry::folder(grana_i18n::app::choose_sequence_folder(
                self.workspace.language,
            )),
            move |app, path| {
                let Some(parent) = path else {
                    return;
                };
                app.export_sequence(parent, stem, width, frames);
            },
        );
    }

    fn export_sequence(
        &mut self,
        parent: std::path::PathBuf,
        stem: String,
        width: u32,
        frames: u32,
    ) {
        if !self.export_ready() {
            return;
        }
        let Some(region) = self.session.scene().map(|scene| scene.region) else {
            return;
        };
        let directory = parent.join(format!("{stem}-sequence"));
        let camera = self.camera.state();
        let poses = (0..frames).map(|time| Pose { time, camera }).collect();
        let size = self.output_size(width, false);
        self.start_export(Plan {
            destination: Destination::Sequence { directory, stem },
            size,
            transparent: self.session.export().transparent,
            poses,
            fps: 0,
            region,
        });
    }

    pub(super) fn cancel_export(&mut self) {
        self.export.cancel();
        self.repaint_at = Some(Instant::now());
    }

    pub(super) fn set_output_message(&mut self, message: &str) {
        self.output = Some(message.to_string());
    }
}
