use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use grana_animation::Keyframe;
use grana_display::Viewport;
use grana_editor::{FrameStats, Pane, Stage, ViewStats, Workspace};
use grana_engine::{Engine, Plan, Pose, ProjectedView, Recording, SAMPLE_LIMIT};
use grana_gpu::Gpu;
use grana_project::ViewState;
use grana_render::FrameInfo;
use grana_session::{Command, Effects, Session, SessionView};
use grana_ui::{
    Button as UiButton, Event as UiEvent, Modifiers as UiModifiers, Rect as UiRect, Ui,
    Vec2 as UiVec2,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const SNAPSHOT_SCALE: u32 = 4;
const SNAPSHOT_LIMIT: u32 = 8192;
const STREAM_POLL: Duration = Duration::from_millis(16);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    ui: Ui,
}

pub struct App {
    surface: Option<Surface>,
    session: Session,
    engine: Option<Engine>,
    start: Instant,
    capture: Option<PathBuf>,
    elapsed_frames: u32,
    exiting: bool,
    viewport: Viewport,
    stage: Stage,
    workspace: Workspace,
    info: FrameInfo,
    generation: u64,
    needs_fit: bool,
    fit_section: bool,
    last_advance: Option<Instant>,
    last_animation: Option<Instant>,
    repaint_at: Option<Instant>,
    title: Option<String>,
    modifiers: ModifiersState,
    message: Option<String>,
}

impl App {
    pub fn new(capture: Option<PathBuf>) -> Self {
        Self {
            surface: None,
            session: Session::new(),
            engine: None,
            start: Instant::now(),
            capture,
            elapsed_frames: 0,
            exiting: false,
            viewport: Viewport::new(),
            stage: Stage::new(),
            workspace: Workspace::default(),
            info: FrameInfo::default(),
            generation: 0,
            needs_fit: true,
            fit_section: false,
            last_advance: None,
            last_animation: None,
            repaint_at: None,
            title: None,
            modifiers: ModifiersState::default(),
            message: None,
        }
    }

    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Grana")
                        .with_visible(self.capture.is_none())
                        .with_inner_size(LogicalSize::new(1440.0, 900.0))
                        .with_min_inner_size(LogicalSize::new(1080.0, 700.0)),
                )
                .expect("failed to create the window"),
        );
        let size = window.inner_size();
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let ui = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        let mut engine = Engine::new(&gpu);
        self.session.open_sample();
        if let Some(scene) = self.session.scene() {
            engine.set_dataset(&gpu, scene.dataset.pyramid());
        }
        self.generation = self.session.generation();
        self.surface = Some(Surface { window, gpu, ui });
        self.engine = Some(engine);
    }

    fn redraw(&mut self) {
        self.session.poll();
        self.exchange_bricks();
        self.session.sync_measure();
        let (commands, animating) = self.run_ui();
        self.interact_viewport();
        if self.apply(commands) {
            return;
        }
        self.session.apply_measure();
        self.sync_title();
        self.advance_playback();
        self.advance_animation();
        self.fit_views();
        let recording = self.engine.as_ref().is_some_and(Engine::recording);
        let mut delay = if recording {
            self.record().unwrap_or(Duration::MAX)
        } else {
            self.render().unwrap_or(Duration::MAX)
        };
        if recording || animating {
            delay = Duration::ZERO;
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.info.samples < SAMPLE_LIMIT {
            delay = Duration::ZERO;
        }
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.workspace.playback.playing {
            delay = delay.min(PLAYBACK_INTERVAL);
        }
        self.schedule(delay);
    }

    fn exchange_bricks(&mut self) {
        let App {
            surface,
            engine,
            session,
            info,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return;
        };
        if let Some(requests) = engine.poll_requests(&surface.gpu) {
            let current = session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time);
            if current {
                info.volume = requests.volume_view;
                info.section = requests.section_view;
                let mut bricks = Vec::with_capacity(
                    requests.volume.len() + requests.section.len() + requests.prefetch.len(),
                );
                bricks.extend_from_slice(&requests.section);
                bricks.extend_from_slice(&requests.volume);
                bricks.extend_from_slice(&requests.prefetch);
                session.request(requests.time, &bricks);
            }
        }
        for (request, data) in session.take_bricks() {
            engine.upload(&surface.gpu, request, &data);
        }
        if session.generation() != self.generation {
            self.generation = session.generation();
            if let Some(scene) = session.scene() {
                engine.set_dataset(&surface.gpu, scene.dataset.pyramid());
            }
            self.needs_fit = true;
            self.fit_section = false;
            self.workspace.playback.playing = false;
            self.workspace.playback.previewing = false;
            self.last_advance = None;
            self.last_animation = None;
            *info = FrameInfo::default();
        }
        let view = session.take_view();
        if let Some(view) = view {
            self.apply_view(&view);
        }
    }

    fn fit_views(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let volume_region = physical_region(self.stage.volume_rect(), scale, size);
        let section_region = physical_region(self.stage.section_rect(), scale, size);
        self.stage.set_regions(volume_region, section_region);
        let volume_aspect = volume_region[2] as f32 / volume_region[3] as f32;
        let section_aspect = section_region[2] as f32 / section_region[3] as f32;
        let sized = volume_region[2] > 64
            && volume_region[3] > 64
            && section_region[2] > 64
            && section_region[3] > 64;
        let Some(scene) = self.session.scene() else {
            return;
        };
        let world = scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size();
        if self.fit_section && sized {
            self.viewport.fit_section(world, voxel, section_aspect);
            self.fit_section = false;
        }
        if self.needs_fit && sized {
            self.viewport
                .fit(world, voxel, volume_aspect, section_aspect);
            self.needs_fit = false;
        }
    }

    fn schedule(&mut self, delay: Duration) {
        self.elapsed_frames += 1;
        let streamed = self.info.volume.resident >= self.info.volume.demand
            && self.info.section.resident >= self.info.section.demand;
        if let Some(path) = self.capture.clone()
            && ((self.elapsed_frames > 30 && streamed && self.info.samples >= SAMPLE_LIMIT)
                || self.elapsed_frames > 150)
        {
            self.capture = None;
            self.write_capture(&path);
            self.exiting = true;
        }
        if delay.is_zero() {
            self.repaint_at = None;
            self.request_redraw();
        } else if delay < MAX_REPAINT_DELAY {
            self.repaint_at = Some(Instant::now() + delay);
        } else {
            self.repaint_at = None;
        }
    }
    fn write_capture(&mut self, path: &Path) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let (pixels, size, format) = surface.ui.renderer_mut().read_color();
        let pixels = if format == wgpu::TextureFormat::Bgra8Unorm {
            let mut swapped = pixels;
            for pixel in swapped.chunks_mut(4) {
                pixel.swap(0, 2);
            }
            swapped
        } else {
            pixels
        };
        match grana_engine::write_rgba(path, size[0], size[1], &pixels) {
            Ok(()) => eprintln!("grana: saved {}", path.display()),
            Err(detail) => eprintln!("{detail}"),
        }
    }

    fn interact_viewport(&mut self) {
        let App {
            surface,
            session,
            viewport,
            stage,
            ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return;
        };
        let spacing = session
            .scene()
            .map_or(grana_volume::Spacing::new(1.0, 1.0, 1.0), |scene| {
                scene.dataset.spacing()
            });
        let interaction = stage.input(&mut surface.ui, viewport, spacing);
        surface
            .window
            .set_cursor(interface_cursor(surface.ui.cursor()));
        if interaction.camera {
            if let Some(scene) = session.scene() {
                viewport.clamp_focus(0.5 * scene.dataset.world_size());
            }
            surface.window.request_redraw();
        }
        let pick = interaction.pick;
        if let Some((pane, point)) = pick {
            self.pick(pane, point);
            self.request_redraw();
        }
    }

    fn apply(&mut self, commands: Vec<Command>) -> bool {
        for command in commands {
            let effects = self.session.execute(command);
            if self.react(effects) {
                return true;
            }
        }
        false
    }

    fn react(&mut self, effects: Effects) -> bool {
        if effects.repaint {
            self.request_redraw();
        }
        if effects.open_dataset
            && let Some(path) = Self::pick_folder()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_image
            && let Some(path) = Self::pick_image()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_project
            && let Some(path) = Self::pick_project()
        {
            self.session.begin_project(path);
            return true;
        }
        if let Some(choose) = effects.save_project {
            let view = self.view_state();
            self.session.save_project(choose, view);
        }
        if effects.snapshot {
            self.begin_snapshot();
        }
        if let Some(width) = effects.export_movie {
            self.begin_movie(width);
        }
        if effects.cancel_movie {
            self.cancel_movie();
        }
        if effects.keyframe {
            self.set_keyframe();
        }
        false
    }

    fn run_ui(&mut self) -> (Vec<Command>, bool) {
        let App {
            surface,
            engine,
            session,
            viewport,
            stage,
            workspace,
            info,
            ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return (Vec::new(), false);
        };
        let Some(engine) = engine.as_mut() else {
            return (Vec::new(), false);
        };
        let Some(scene) = session.scene() else {
            return (Vec::new(), false);
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let points = UiVec2::new(size[0] as f32 / scale, size[1] as f32 / scale);
        surface.ui.resize(size);
        surface
            .ui
            .begin(points, scale, self.start.elapsed().as_secs_f64());
        let adapter = surface.gpu.adapter.clone();
        let movie = engine.movie();
        let movie_message = engine.movie_message();
        let view = SessionView {
            scene,
            measure: session.measure(),
            timeline: session.timeline(),
            jobs: session.progress(),
            io: session.io(&adapter, movie, movie_message),
            batch: session.batch(),
            history: session.history(),
        };
        let frame = frame_stats(*info);
        let commands =
            grana_editor::build(&mut surface.ui, &view, viewport, stage, workspace, frame);
        let frame = surface.ui.end();
        (commands, frame.animating)
    }

    fn render(&mut self) -> Option<Duration> {
        if self.engine.as_ref().is_some_and(Engine::snapshot_pending) {
            let delay = self.capture();
            self.present_ui();
            return delay;
        }
        let App {
            surface,
            engine,
            session,
            viewport,
            stage,
            ..
        } = self;
        let surface = surface.as_mut()?;
        let engine = engine.as_mut()?;
        let scene = session.scene()?;
        let size = surface.gpu.size();
        let scale = surface.window.scale_factor() as f32;
        let volume_region = physical_region(stage.volume_rect(), scale, size);
        let section_region = physical_region(stage.section_rect(), scale, size);
        let frame = surface.gpu.acquire()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let playing = self.workspace.playback.playing;
        let info = engine.draw_viewport(
            &surface.gpu,
            grana_engine::ViewportFrame {
                target: surface.ui.scene_view(),
                scene,
                viewport,
                size,
                volume_region,
                section_region,
                prefetch: playing,
            },
        );
        let mut encoder =
            surface
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grana.ui.paint"),
                });
        surface.ui.paint(&mut encoder, &view);
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
        self.info = info;
        Some(Duration::MAX)
    }

    fn capture(&mut self) -> Option<Duration> {
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let surface = surface.as_mut()?;
        let engine = engine.as_mut()?;
        let scene = session.scene()?;
        let size = surface.gpu.size();
        let scale = surface.window.scale_factor() as f32;
        let pane = physical_region(self.stage.volume_rect(), scale, size);
        let aspect = self.stage.volume_rect().width() / self.stage.volume_rect().height().max(1.0);
        let pixels_per_point = self.stage.volume_rect().width().max(1.0);
        let screen = pane[2].max(pane[3]).max(1);
        let projected = ProjectedView {
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
            aspect,
            world_per_point: 2.0
                * self.viewport.camera().distance()
                * (self.viewport.camera().fov_y() * 0.5).tan()
                / self.stage.volume_rect().height().max(1.0),
            pixels_per_point: screen as f32 / pixels_per_point,
        };
        engine.capture(&surface.gpu, &mut surface.ui, scene, &projected);
        Some(Duration::ZERO)
    }

    fn begin_snapshot(&mut self) {
        if self.engine.as_ref().is_some_and(Engine::recording) {
            return;
        }
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine), Some(scene)) =
            (surface.as_mut(), engine.as_mut(), session.scene())
        else {
            return;
        };
        let pixels = surface.window.scale_factor() as f32;
        let pane = physical_region(self.stage.volume_rect(), pixels, surface.gpu.size());
        let longest = pane[2].max(pane[3]).max(1);
        let scale = SNAPSHOT_SCALE.min((SNAPSHOT_LIMIT / longest).max(1));
        let size = [(pane[2] * scale).max(1), (pane[3] * scale).max(1)];
        let suggested = format!("{}-snapshot.png", grana_session::slug(scene.dataset.name()));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a snapshot")
            .set_file_name(suggested)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        engine.begin_snapshot(&surface.gpu, path, size);
    }

    fn begin_movie(&mut self, width: u32) {
        if self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.recording() || engine.snapshot_pending())
            || self.session.loading().is_some()
        {
            return;
        }
        if self.session.timeline().is_empty() {
            if let Some(engine) = self.engine.as_mut() {
                engine.set_movie_message(Some("set a keyframe before exporting".to_string()));
            }
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let suggested = format!("{}-movie.mp4", grana_session::slug(scene.dataset.name()));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a movie")
            .set_file_name(suggested)
            .add_filter("MP4", &["mp4"])
            .save_file()
        else {
            return;
        };
        self.start_movie(path, width);
    }

    fn start_movie(&mut self, path: PathBuf, width: u32) {
        let position = self.workspace.playback.position;
        let layout = self.movie_layout(width);
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine), Some(scene)) =
            (surface.as_mut(), engine.as_mut(), session.scene())
        else {
            return;
        };
        if engine.recording() || engine.snapshot_pending() || session.loading().is_some() {
            return;
        }
        if session.timeline().is_empty() {
            engine.set_movie_message(Some("set a keyframe before exporting".to_string()));
            return;
        }
        let Some((size, volume)) = layout else {
            return;
        };
        let limit = surface.gpu.device.limits().max_texture_dimension_2d;
        if size[0] > limit || size[1] > limit {
            engine.set_movie_message(Some(format!(
                "a {} × {} movie exceeds the {limit} pixel limit of this device",
                size[0], size[1]
            )));
            return;
        }
        let plan = Plan {
            path,
            size,
            volume,
            fps: session.timeline().fps(),
            frames: session.timeline().frames(),
            resume: scene.time(),
            position,
        };
        match engine.start_movie(&surface.gpu, plan) {
            Ok(()) => {
                self.workspace.playback.previewing = false;
                self.workspace.playback.playing = false;
                self.last_advance = None;
                self.last_animation = None;
                self.message = None;
                self.repaint_at = Some(Instant::now());
            }
            Err(detail) => self.message = Some(detail),
        }
    }

    fn cancel_movie(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if let Some((time, position)) = engine.cancel_movie() {
            self.session.set_time(time);
            self.workspace.playback.position =
                position.clamp(0.0, self.session.timeline().duration());
        }
        self.repaint_at = Some(Instant::now());
    }

    fn record(&mut self) -> Option<Duration> {
        let Some(frame) = self.engine.as_ref().and_then(Engine::movie) else {
            self.present_ui();
            return Some(Duration::ZERO);
        };
        let Some(sample) = self.session.timeline().sample_frame(frame.0) else {
            if let Some(engine) = self.engine.as_mut() {
                engine
                    .set_movie_message(Some("the timeline ended before the movie did".to_string()));
            }
            self.cancel_movie();
            self.present_ui();
            return Some(Duration::ZERO);
        };
        self.session.set_time(sample.time);
        let pose = Pose {
            time: sample.time,
            position: self.workspace.playback.position,
            camera: sample.camera,
            section: sample.section,
        };
        let outcome = {
            let App {
                surface,
                engine,
                session,
                ..
            } = self;
            let (Some(surface), Some(engine), Some(scene)) =
                (surface.as_mut(), engine.as_mut(), session.scene())
            else {
                return Some(Duration::ZERO);
            };
            engine.record(&surface.gpu, scene, &pose)
        };
        self.present_ui();
        match outcome {
            Recording::Running => {}
            Recording::Complete { path } => {
                if let Some(engine) = self.engine.as_mut() {
                    engine.set_movie_message(Some(format!("saved {}", grana_engine::name(&path))));
                }
                self.restore_time(pose.time, pose.position);
            }
            Recording::Failed { detail } => {
                self.message = Some(detail);
                self.restore_time(pose.time, pose.position);
            }
        }
        Some(Duration::ZERO)
    }

    fn restore_time(&mut self, time: u32, position: f32) {
        self.session.set_time(time);
        self.workspace.playback.position = position.clamp(0.0, self.session.timeline().duration());
    }

    fn present_ui(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(frame) = surface.gpu.acquire() else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            surface
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grana.ui.present"),
                });
        surface.ui.paint(&mut encoder, &view);
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
    }

    fn movie_layout(&self, width: u32) -> Option<([u32; 2], [u32; 4])> {
        let surface = self.surface.as_ref()?;
        let scale = surface.window.scale_factor() as f32;
        let surface_size = surface.gpu.size();
        let pane = physical_region(self.stage.volume_rect(), scale, surface_size);
        let factor = width as f32 / pane[2] as f32;
        let size = [
            even((pane[2] as f32 * factor).round() as u32),
            even((pane[3] as f32 * factor).round() as u32),
        ];
        Some((size, scale_region(pane, [pane[0], pane[1]], factor, size)))
    }

    fn streaming(&self) -> bool {
        let Some(engine) = self.engine.as_ref() else {
            return false;
        };
        self.session.batch().is_some_and(|batch| batch.running)
            || self.workspace.playback.playing
            || self.session.loading().is_some()
            || !self.session.progress().is_empty()
            || self.session.pending() > 0
            || engine.requests_pending()
            || engine.recording()
            || engine.snapshot_pending()
            || (self.session.scene().is_some()
                && (self.info.volume.resident < self.info.volume.demand
                    || self.info.section.resident < self.info.section.demand))
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
            split: self.viewport.split(),
            timeline: self.session.timeline().clone(),
        }
    }

    fn apply_view(&mut self, view: &ViewState) {
        self.viewport.restore(view.camera, view.section);
        self.viewport.set_split(view.split);
        self.session.restore_timeline(view.timeline.clone());
        self.workspace.playback.selected = None;
        self.workspace.playback.position = 0.0;
        self.needs_fit = false;
    }

    fn dirty(&self) -> bool {
        self.session.dirty()
    }

    fn sync_title(&mut self) {
        let name = self
            .session
            .project_name()
            .unwrap_or_else(|| "untitled".to_string());
        let marker = if self.dirty() { " •" } else { "" };
        let title = format!("Grana — {name}{marker}");
        if self.title.as_deref() == Some(title.as_str()) {
            return;
        }
        self.title = Some(title.clone());
        if let Some(surface) = &self.surface {
            surface.window.set_title(&title);
        }
    }

    fn pick(&mut self, pane: Pane, point: Vec2) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let ndc = self.stage.ndc(pane, point);
        let aspect = self.stage.aspect(pane);
        let selection = match pane {
            Pane::Volume => {
                let ray = self.viewport.camera().ray(ndc, aspect);
                scene.pick_ray(&ray)
            }
            Pane::Section => {
                let section = self.viewport.section();
                let axis = section.axis();
                let focus = self.viewport.camera().focus();
                let mut target = section.point(focus, ndc, aspect);
                let extent = scene.dataset.extent();
                let world = scene.dataset.world_size()[axis.index()];
                let layers = axis.extent(extent) as f32;
                let layer = ((focus[axis.index()] / world + 0.5) * layers)
                    .floor()
                    .clamp(0.0, layers - 1.0);
                target[axis.index()] = ((layer + 0.5) / layers - 0.5) * world;
                scene.pick_slice(axis, target)
            }
        };
        self.session.execute(Command::Select(selection));
        self.request_redraw();
    }

    fn set_keyframe(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let duration = self.session.timeline().duration();
        let at = self.workspace.playback.position.clamp(0.0, duration);
        let key = Keyframe {
            at,
            time: scene.time(),
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
        };
        let index = self.session.insert_keyframe(key);
        self.workspace.playback.position = at;
        self.workspace.playback.selected = Some(index);
        if let Some(engine) = self.engine.as_mut() {
            engine.set_movie_message(None);
        }
        self.request_redraw();
    }

    fn advance_playback(&mut self) {
        if !self.workspace.playback.playing {
            return;
        }
        let Some(scene) = self.session.scene() else {
            self.workspace.playback.playing = false;
            return;
        };
        if scene.frame_count() <= 1 {
            self.workspace.playback.playing = false;
            return;
        }
        let now = Instant::now();
        let due = self
            .last_advance
            .is_none_or(|last| now.duration_since(last) >= PLAYBACK_INTERVAL);
        if !due {
            return;
        }
        let next = (scene.time() + 1) % scene.frame_count();
        self.session.set_time(next);
        self.workspace.playback.previewing = false;
        self.last_advance = Some(now);
    }

    fn advance_animation(&mut self) {
        if !self.workspace.playback.previewing {
            return;
        }
        let duration = self.session.timeline().duration();
        let now = Instant::now();
        if let Some(last) = self.last_animation.replace(now) {
            let elapsed = now.duration_since(last).as_secs_f32().min(0.25);
            self.workspace.playback.position =
                (self.workspace.playback.position + elapsed) % duration;
        }
        self.pose_animation();
    }

    fn pose_animation(&mut self) {
        let position = self.workspace.playback.position;
        if let Some(sample) = self.session.timeline().sample(position) {
            self.viewport.restore(sample.camera, sample.section);
            self.session.set_time(sample.time);
        }
    }

    fn toggle_play(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        if scene.frame_count() <= 1 {
            return;
        }
        let playing = !self.workspace.playback.playing;
        self.workspace.playback.playing = playing;
        self.last_advance = playing.then(Instant::now);
        if playing {
            self.workspace.playback.previewing = false;
            self.last_animation = None;
        }
    }

    fn step_time(&mut self, delta: i64) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let frames = scene.frame_count();
        if frames <= 1 {
            return;
        }
        let time = (i64::from(scene.time()) + delta).rem_euclid(i64::from(frames)) as u32;
        self.workspace.playback.playing = false;
        self.workspace.playback.previewing = false;
        self.last_advance = None;
        self.last_animation = None;
        self.session.set_time(time);
    }

    fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr folder")
            .pick_folder()
    }

    fn pick_image() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a microscopy image")
            .add_filter(
                "Microscopy image",
                &["czi", "ome.tif", "ome.tiff", "tif", "tiff"],
            )
            .add_filter("ZEISS CZI", &["czi"])
            .add_filter("OME-TIFF", &["ome.tif", "ome.tiff", "tif", "tiff"])
            .pick_file()
    }

    fn pick_project() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a grana project")
            .add_filter("grana project", &["grana"])
            .pick_file()
    }

    fn on_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        if shortcut(self.modifiers) {
            if let Key::Character(text) = &event.logical_key {
                if text.as_str().eq_ignore_ascii_case("s") {
                    let view = self.view_state();
                    self.session.save_project(self.modifiers.shift_key(), view);
                } else if text.as_str().eq_ignore_ascii_case("o")
                    && let Some(path) = Self::pick_project()
                {
                    self.session.begin_project(path);
                } else if text.as_str().eq_ignore_ascii_case("z") {
                    let command = if self.modifiers.shift_key() {
                        Command::Redo
                    } else {
                        Command::Undo
                    };
                    self.apply(vec![command]);
                } else if text.as_str().eq_ignore_ascii_case("y") {
                    self.apply(vec![Command::Redo]);
                }
            }
            self.request_redraw();
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
            _ => return,
        }
        self.request_redraw();
    }

    fn confirm_close(&self) -> bool {
        rfd::MessageDialog::new()
            .set_title("Unsaved analysis")
            .set_description("The project has unsaved changes. Close grana without saving them?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    fn request_redraw(&self) {
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn pump_capture(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            return;
        }
        if self
            .repaint_at
            .is_none_or(|deadline| Instant::now() >= deadline)
        {
            self.repaint_at = None;
            self.redraw();
        }
        if self.exiting {
            event_loop.exit();
            return;
        }
        match self.repaint_at {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Poll),
        }
    }
}

fn frame_stats(info: FrameInfo) -> FrameStats {
    FrameStats {
        volume: view_stats(info.volume),
        section: view_stats(info.section),
        bytes: info.bytes,
        geometry: info.geometry,
        samples: info.samples,
    }
}

fn view_stats(info: grana_render::ViewInfo) -> ViewStats {
    ViewStats {
        level: info.level,
        visible: info.visible,
        demand: info.demand,
        resident: info.resident,
    }
}

fn physical_region(rect: UiRect, scale: f32, surface: [u32; 2]) -> [u32; 4] {
    let x = ((rect.min.x * scale).round() as i64).clamp(0, surface[0] as i64 - 1) as u32;
    let y = ((rect.min.y * scale).round() as i64).clamp(0, surface[1] as i64 - 1) as u32;
    let width = ((rect.width() * scale).round() as u32)
        .min(surface[0] - x)
        .max(1);
    let height = ((rect.height() * scale).round() as u32)
        .min(surface[1] - y)
        .max(1);
    [x, y, width, height]
}

fn interface_key(key: &Key) -> Option<grana_ui::Key> {
    match key {
        Key::Named(NamedKey::Escape) => Some(grana_ui::Key::Escape),
        Key::Named(NamedKey::Enter) => Some(grana_ui::Key::Enter),
        Key::Named(NamedKey::Tab) => Some(grana_ui::Key::Tab),
        Key::Named(NamedKey::Space) => Some(grana_ui::Key::Space),
        Key::Named(NamedKey::ArrowLeft) => Some(grana_ui::Key::Left),
        Key::Named(NamedKey::ArrowRight) => Some(grana_ui::Key::Right),
        Key::Named(NamedKey::ArrowUp) => Some(grana_ui::Key::Up),
        Key::Named(NamedKey::ArrowDown) => Some(grana_ui::Key::Down),
        Key::Named(NamedKey::Home) => Some(grana_ui::Key::Home),
        Key::Named(NamedKey::End) => Some(grana_ui::Key::End),
        Key::Named(NamedKey::PageUp) => Some(grana_ui::Key::PageUp),
        Key::Named(NamedKey::PageDown) => Some(grana_ui::Key::PageDown),
        Key::Named(NamedKey::Backspace) => Some(grana_ui::Key::Backspace),
        Key::Named(NamedKey::Delete) => Some(grana_ui::Key::Delete),
        Key::Character(text) => text.chars().next().map(grana_ui::Key::Character),
        _ => None,
    }
}

fn interface_cursor(cursor: grana_ui::Cursor) -> CursorIcon {
    match cursor {
        grana_ui::Cursor::Arrow => CursorIcon::Default,
        grana_ui::Cursor::Pointing => CursorIcon::Pointer,
        grana_ui::Cursor::Crosshair => CursorIcon::Crosshair,
        grana_ui::Cursor::Grab => CursorIcon::Grab,
        grana_ui::Cursor::Grabbing => CursorIcon::Grabbing,
        grana_ui::Cursor::ResizeHorizontal => CursorIcon::ColResize,
        grana_ui::Cursor::ResizeVertical => CursorIcon::RowResize,
        grana_ui::Cursor::Move => CursorIcon::Move,
    }
}

fn even(value: u32) -> u32 {
    (value.max(2) / 2) * 2
}

fn shortcut(modifiers: ModifiersState) -> bool {
    modifiers.control_key() || modifiers.super_key()
}

fn scale_region(region: [u32; 4], origin: [u32; 2], factor: f32, size: [u32; 2]) -> [u32; 4] {
    let x = ((region[0].saturating_sub(origin[0]) as f32 * factor).round() as u32).min(size[0] - 1);
    let y = ((region[1].saturating_sub(origin[1]) as f32 * factor).round() as u32).min(size[1] - 1);
    let width = ((region[2] as f32 * factor).round() as u32).clamp(1, size[0] - x);
    let height = ((region[3] as f32 * factor).round() as u32).clamp(1, size[1] - y);
    [x, y, width, height]
}

impl App {
    fn push_event(&mut self, event: &WindowEvent) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let input = surface.ui.input_mut();
        match event {
            WindowEvent::CursorMoved { position, .. } => input.push(UiEvent::Moved(UiVec2::new(
                position.x as f32 / scale,
                position.y as f32 / scale,
            ))),
            WindowEvent::CursorLeft { .. } => input.push(UiEvent::Leave),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => UiButton::Primary,
                    MouseButton::Right => UiButton::Secondary,
                    MouseButton::Middle => UiButton::Middle,
                    _ => return,
                };
                input.push(match state {
                    ElementState::Pressed => UiEvent::Down(button),
                    ElementState::Released => UiEvent::Up(button),
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => UiVec2::new(*x, *y),
                    MouseScrollDelta::PixelDelta(position) => {
                        UiVec2::new(position.x as f32, position.y as f32) / 48.0
                    }
                };
                input.push(UiEvent::Wheel(lines));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                input.push(UiEvent::Modifiers(UiModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    logo: state.super_key(),
                }));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(key) = interface_key(&event.logical_key) else {
                    return;
                };
                input.push(UiEvent::Key {
                    key,
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                });
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            self.resume(event_loop);
        }
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.surface.is_none() {
            return;
        }
        self.push_event(&event);
        match event {
            WindowEvent::CloseRequested => {
                if !self.dirty() || self.confirm_close() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.gpu.resize([size.width, size.height]);
                    surface.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if path
                    .extension()
                    .is_some_and(|extension| extension == "grana")
                {
                    self.session.begin_project(path);
                } else if grana_session::is_image(&path) {
                    self.session.begin_load(path);
                } else {
                    self.message = Some(format!(
                        "{} is neither a supported image nor a grana project",
                        path.display()
                    ));
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(&event);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            event_loop.exit();
            return;
        }
        if self.capture.is_some() {
            self.pump_capture(event_loop);
            return;
        }
        if self.repaint_at.is_none() {
            if self.workspace.playback.playing {
                self.repaint_at = Some(Instant::now() + PLAYBACK_INTERVAL);
            } else if self.streaming() {
                self.repaint_at = Some(Instant::now() + STREAM_POLL);
            }
        }
        let Some(deadline) = self.repaint_at else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if Instant::now() >= deadline {
            self.repaint_at = None;
            if let Some(surface) = &self.surface {
                surface.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }
}
