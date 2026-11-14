use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_display::Camera;
use grana_editor::{Frame, Stage, Workspace};
use grana_engine::{Engine, SAMPLE_LIMIT};
use grana_gpu::Gpu;
use grana_i18n::Language;
use grana_scene::Revision;
use grana_session::{Access, Command, Session};
use grana_ui::Ui;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

mod chrome;
mod command;
mod editor;
mod export;
mod frame;
mod input;
mod overlay;
mod sync;
mod time;
mod viewport;

use self::export::Export;
use crate::clipboard::SystemClipboard;
use crate::preference::Preference;
use crate::stream::Stream;
use crate::window;

const STREAM_POLL: Duration = Duration::from_millis(16);
const EXPORT_POLL: Duration = Duration::from_millis(200);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const PLAYBACK_POLL: Duration = Duration::from_millis(4);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);
const CAPTURE_BUDGET: Duration = Duration::from_secs(240);
const TOOL_CLICK_SLOP: f32 = 3.0;
const AUTOSAVE_IDLE: Duration = Duration::from_secs(8);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);
const EDITOR_SIZE: LogicalSize<f64> = LogicalSize::new(1440.0, 900.0);
const EDITOR_MINIMUM: LogicalSize<f64> = LogicalSize::new(1080.0, 700.0);
const STARTUP_MINIMUM: LogicalSize<f64> = LogicalSize::new(360.0, 260.0);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowFit {
    Startup,
    Editor,
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
    interacting: bool,
    animating: bool,
    chrome: window::Chrome,
    closing: bool,
    window_fit: WindowFit,
    window_fitted: bool,
    reveal: bool,
}

impl App {
    pub fn new(capture: Option<PathBuf>, startup: Option<String>, access: Access) -> Self {
        let mut session = match access {
            Access::Author => Session::new(),
            Access::Viewer => Session::viewer(),
        };
        session.attach_recent_store();
        if access.author() {
            session.attach_recovery_store();
        }
        session.set_capture_factory(Arc::new(|| Arc::new(crate::capture::OfflineCapture::new())));
        let preference = Preference::load();
        let language = Language::from_environment().unwrap_or(preference.language);
        let mut workspace = Workspace::default();
        workspace.language = language;
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
            interacting: false,
            animating: false,
            chrome: window::Chrome::new(),
            closing: false,
            window_fit: WindowFit::Startup,
            window_fitted: false,
            reveal: false,
        }
    }

    pub(super) fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let icon = winit::window::Icon::from_rgba(grana_brand::icon(256), 256, 256)
            .expect("the brand mark rasterizes to a window icon");
        let window = Arc::new(
            event_loop
                .create_window(window::attributes(
                    Window::default_attributes()
                        .with_title("Grana")
                        .with_window_icon(Some(icon))
                        .with_visible(false)
                        .with_inner_size(EDITOR_SIZE)
                        .with_min_inner_size(STARTUP_MINIMUM),
                ))
                .expect("failed to create the window"),
        );
        let size = window.inner_size();
        window.set_ime_allowed(false);
        window.set_theme(Some(winit::window::Theme::Dark));
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let mut ui: Ui<grana_editor::Message> =
            Ui::new(&gpu.device, &gpu.queue, gpu.format(), gpu.pipelines());
        ui.set_clipboard(Box::new(SystemClipboard::new()));
        let mut engine = Engine::new(&gpu.device, &gpu.queue, gpu.format(), gpu.pipelines());
        if let Some(target) = self.startup.take() {
            if grana_session::is_remote(&target) {
                self.session.begin_remote(target);
            } else {
                self.session.open_path(PathBuf::from(target));
            }
        }
        if let Some(scene) = self.session.scene() {
            engine.set_dataset(&gpu.device, scene.dataset.pyramid());
        }
        self.replaced = self.session.replaced();
        self.content = self.session.content();
        self.surface = Some(Surface { window, gpu, ui });
        self.engine = Some(engine);
    }

    pub(super) fn redraw(&mut self) {
        self.session.poll();
        if self.follow_resume() {
            return;
        }
        self.sync_window_fit();
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
        let (frame, animating) = self.run_ui();
        self.fit_window(&frame);
        self.animating = animating;
        self.sync_preference();
        self.sync_ime();
        self.interact_viewport();
        self.sync_window_cursor();
        if self.apply(frame.commands) {
            return;
        }
        self.perform_window(frame.window);
        self.session.apply_measure();
        self.sync_title();
        self.autosave();
        self.advance_playback();
        self.advance_animation();
        let display = self.display_mode(animating);
        self.interacting = false;
        self.fit_view();
        let mut delay = self.render(display).unwrap_or(Duration::MAX);
        if animating {
            delay = Duration::ZERO;
        } else if self.workspace.playback.playing {
            delay = delay.min(PLAYBACK_POLL);
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.session.scene().is_some() && self.converging() {
            delay = Duration::ZERO;
        }
        if !self.workspace.playback.playing && self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.session.stitching() {
            delay = delay.min(STREAM_POLL);
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

    fn sync_window_fit(&mut self) {
        let fit = if self.session.scene().is_some() {
            WindowFit::Editor
        } else {
            WindowFit::Startup
        };
        if self.window_fit != fit {
            self.window_fit = fit;
            self.window_fitted = false;
        }
    }

    fn fit_window(&mut self, frame: &Frame) {
        if self.window_fitted {
            return;
        }
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let window = &surface.window;
        match self.window_fit {
            WindowFit::Startup => {
                let Some(size) = frame.startup else {
                    return;
                };
                let size = LogicalSize::new(size.x as f64, size.y as f64);
                window.set_min_inner_size(Some(size));
                let current = window.inner_size().to_logical::<f64>(window.scale_factor());
                let settled = (current.width - size.width).abs() < 1.0
                    && (current.height - size.height).abs() < 1.0;
                if settled {
                    if self.capture.is_none() {
                        window.set_visible(true);
                    }
                } else {
                    let _ = window.request_inner_size(size);
                    self.reveal = self.capture.is_none();
                }
            }
            WindowFit::Editor => {
                window.set_min_inner_size(Some(EDITOR_MINIMUM));
                let scale = window.scale_factor();
                let current = window.inner_size().to_logical::<f64>(scale);
                if current.width < EDITOR_SIZE.width || current.height < EDITOR_SIZE.height {
                    let _ = window.request_inner_size(LogicalSize::new(
                        current.width.max(EDITOR_SIZE.width),
                        current.height.max(EDITOR_SIZE.height),
                    ));
                }
            }
        }
        if self.capture.is_none() && self.window_fit == WindowFit::Editor {
            window.set_visible(true);
        }
        self.window_fitted = true;
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
        self.track_window(&event);
        if let Some(press) = self.claim_window(&event) {
            self.apply_window_press(press);
            return;
        }
        self.push_event(&event);
        match event {
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.gpu.resize([size.width, size.height]);
                    self.chrome.refresh(&surface.window);
                    surface.window.request_redraw();
                }
                if self.reveal {
                    self.reveal = false;
                    if let Some(surface) = self.surface.as_ref() {
                        surface.window.set_visible(true);
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    self.chrome.refresh(&surface.window);
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
        if self.closing {
            self.closing = false;
            self.request_close(event_loop);
            if self.exiting {
                event_loop.exit();
                return;
            }
        }
        if self.capture.is_some() {
            self.pump_capture(event_loop);
            return;
        }
        if self.repaint_at.is_none() {
            if self.workspace.playback.playing {
                self.repaint_at = Some(Instant::now() + PLAYBACK_POLL);
            } else if self.streaming() {
                self.repaint_at = Some(Instant::now() + STREAM_POLL);
            } else if self.export.running() {
                self.repaint_at = Some(Instant::now() + EXPORT_POLL);
            } else if self.capture.is_none()
                && self.session.access().author()
                && self.session.dirty()
            {
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
