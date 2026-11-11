use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_display::Camera;
use grana_editor::{Stage, Workspace};
use grana_engine::{Engine, SAMPLE_LIMIT};
use grana_gpu::Gpu;
use grana_i18n::Language;
use grana_scene::Revision;
use grana_session::{Command, Session};
use grana_ui::Ui;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

mod command;
mod editor;
mod export;
mod frame;
mod input;
mod sync;
mod time;
mod viewport;

use self::export::Export;
use crate::clipboard::SystemClipboard;
use crate::preference::Preference;
use crate::stream::Stream;

const STREAM_POLL: Duration = Duration::from_millis(16);
const EXPORT_POLL: Duration = Duration::from_millis(200);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);
const CAPTURE_BUDGET: Duration = Duration::from_secs(240);
const TOOL_CLICK_SLOP: f32 = 3.0;
const AUTOSAVE_IDLE: Duration = Duration::from_secs(8);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);
struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    ui: Ui<grana_editor::Message>,
}

#[derive(Clone)]
enum Resume {
    Exit,
    Command(Command),
}

pub struct App {
    surface: Option<Surface>,
    session: Session,
    engine: Option<Engine>,
    start: Instant,
    capture: Option<PathBuf>,
    startup: Option<String>,
    elapsed_frames: u32,
    exiting: bool,
    camera: Camera,
    stage: Stage,
    workspace: Workspace,
    replaced: Revision,
    content: Revision,
    needs_fit: bool,
    last_advance: Option<Instant>,
    last_animation: Option<Instant>,
    repaint_at: Option<Instant>,
    title: Option<String>,
    modifiers: ModifiersState,
    output: Option<String>,
    ime: bool,
    ime_area: Option<(i32, i32, u32, u32)>,
    stream: Stream,
    pick_toggle: bool,
    mask_path: Vec<[f32; 3]>,
    plate: Option<(String, usize)>,
    export: Export,
    resume: Option<Resume>,
    autosave_at: Option<Instant>,
    changed_at: Option<Instant>,
    revision: Revision,
    language: Language,
    contrast: bool,
    reduce_transparency: bool,
    interacting: bool,
    animating: bool,
}

impl App {
    pub fn new(capture: Option<PathBuf>, startup: Option<String>) -> Self {
        let mut session = Session::new();
        session.attach_recent_store();
        session.attach_recovery_store();
        session.set_capture_factory(Arc::new(|| Arc::new(crate::capture::OfflineCapture::new())));
        let preference = Preference::load();
        let language = Language::from_environment().unwrap_or(preference.language);
        let mut workspace = Workspace::default();
        workspace.language = language;
        workspace.contrast = preference.contrast;
        workspace.reduce_transparency = preference.reduce_transparency;
        Self {
            surface: None,
            session,
            engine: None,
            start: Instant::now(),
            capture,
            startup,
            elapsed_frames: 0,
            exiting: false,
            camera: Camera::new(35.0),
            stage: Stage::new(),
            workspace,
            replaced: Revision::ZERO,
            content: Revision::ZERO,
            needs_fit: true,
            last_advance: None,
            last_animation: None,
            repaint_at: None,
            title: None,
            modifiers: ModifiersState::default(),
            output: None,
            ime: false,
            ime_area: None,
            stream: Stream::new(),
            pick_toggle: false,
            mask_path: Vec::new(),
            plate: None,
            export: Export::new(),
            resume: None,
            autosave_at: None,
            changed_at: None,
            revision: Revision::ZERO,
            language,
            contrast: preference.contrast,
            reduce_transparency: preference.reduce_transparency,
            interacting: false,
            animating: false,
        }
    }

    pub(super) fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let icon = winit::window::Icon::from_rgba(grana_brand::icon(256), 256, 256)
            .expect("the brand mark rasterizes to a window icon");
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Grana")
                        .with_window_icon(Some(icon))
                        .with_visible(self.capture.is_none())
                        .with_inner_size(LogicalSize::new(1440.0, 900.0))
                        .with_min_inner_size(LogicalSize::new(1080.0, 700.0)),
                )
                .expect("failed to create the window"),
        );
        let size = window.inner_size();
        window.set_ime_allowed(false);
        window.set_theme(Some(winit::window::Theme::Dark));
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let mut ui: Ui<grana_editor::Message> = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        ui.set_clipboard(Box::new(SystemClipboard::new()));
        let mut engine = Engine::new(&gpu.device, &gpu.queue, gpu.format());
        if let Some(target) = self.startup.take() {
            if grana_session::is_remote(&target) {
                self.session.begin_remote(target);
            } else {
                let path = PathBuf::from(target);
                if grana_session::is_project(&path) {
                    self.session.begin_project(path);
                } else {
                    self.session.begin_load(path);
                }
            }
        }
        if let Some(scene) = self.session.scene() {
            engine.set_dataset(&gpu.device, scene.dataset.pyramid());
        }
        self.replaced = self.session.replaced();
        self.content = self.session.content();
        self.surface = Some(Surface { window, gpu, ui });
        self.engine = Some(engine);
        self.sync_theme();
    }

    pub(super) fn redraw(&mut self) {
        self.session.poll();
        if self.follow_resume() {
            return;
        }
        self.track_changes();
        self.poll_export();
        self.sync_plate_panel();
        if let Some(set) = self.workspace.seed
            && self
                .session
                .scene()
                .is_none_or(|scene| scene.objects.get(set).is_none())
        {
            self.workspace.seed = None;
        }
        self.exchange_bricks();
        self.poll_pick();
        self.session.sync_measure();
        let (commands, animating) = self.run_ui();
        self.animating = animating;
        self.sync_preference();
        self.sync_ime();
        self.interact_viewport();
        if self.apply(commands) {
            return;
        }
        self.session.apply_measure();
        self.sync_title();
        self.autosave();
        self.advance_playback();
        self.advance_animation();
        let interacting = animating || self.interacting || self.workspace.playback.previewing;
        self.interacting = false;
        self.fit_view();
        let mut delay = self.render(interacting).unwrap_or(Duration::MAX);
        if animating {
            delay = Duration::ZERO;
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.session.scene().is_some() && self.converging() {
            delay = Duration::ZERO;
        }
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.workspace.playback.playing {
            delay = delay.min(PLAYBACK_INTERVAL);
        }
        if self.export.running() {
            delay = delay.min(EXPORT_POLL);
        }
        self.schedule(delay);
    }

    pub(super) fn schedule(&mut self, delay: Duration) {
        self.elapsed_frames += 1;
        let info = self.frame_info();
        let loaded = self.session.scene().is_some();
        let streamed = info.view.resident >= info.view.demand;
        let settled = loaded
            && streamed
            && info.samples >= SAMPLE_LIMIT
            && !info.fading
            && !self.session.previewing();
        let occupied = self.streaming();
        if let Some(path) = self.capture.clone()
            && self.elapsed_frames > 30
            && (!self.animating || self.start.elapsed() > CAPTURE_BUDGET)
            && (settled || (!occupied && !loaded) || self.start.elapsed() > CAPTURE_BUDGET)
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
    pub(super) fn write_capture(&mut self, path: &Path) {
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
        let raster = grana_raster::Raster::new(size[0], size[1], pixels);
        match grana_raster::write(path, &raster) {
            Ok(()) => eprintln!("grana: saved {}", path.display()),
            Err(detail) => eprintln!("{detail}"),
        }
    }

    pub(super) fn request_redraw(&self) {
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    pub(super) fn pump_capture(&mut self, event_loop: &ActiveEventLoop) {
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
                if self.capture.is_some() {
                    event_loop.exit();
                    return;
                }
                if self.export.running() {
                    self.confirm_export_close();
                    return;
                }
                if self.release_document(Resume::Exit) {
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
            WindowEvent::Moved(_) => {}
            WindowEvent::DroppedFile(path) => {
                self.apply(vec![Command::OpenPath(path)]);
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
            } else if self.export.running() {
                self.repaint_at = Some(Instant::now() + EXPORT_POLL);
            } else if self.capture.is_none() && self.session.dirty() {
                self.repaint_at = Some(Instant::now() + self.autosave_wait());
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
