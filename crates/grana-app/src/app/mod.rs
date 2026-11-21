use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use grana_display::Camera;
use grana_editor::{Frame, Stage, Workspace};
use grana_engine::Engine;
use grana_gpu::Gpu;
use grana_i18n::Language;
use grana_scene::Revision;
use grana_session::{Access, Command, Session};
use grana_ui::Ui;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

mod chrome;
mod command;
mod dialog;
mod editor;
mod export;
mod fail;
mod frame;
mod input;
mod placement;
mod release;
mod surface;
mod sync;
mod time;
mod viewport;

use self::dialog::{Awaiting, Service};
use self::export::Export;
use self::placement::{Fit, Placement};
use self::release::{Deferred, Release};
use self::surface::Surface;
use self::viewport::AnnotationKey;
use crate::clipboard::SystemClipboard;
use crate::preference::Preference;
use crate::stream::Stream;
use crate::window;

const STREAM_POLL: Duration = Duration::from_millis(16);
const EXPORT_POLL: Duration = Duration::from_millis(200);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const INTERACTION_HOLD: Duration = Duration::from_millis(220);
const RETRY_INTERVAL: Duration = Duration::from_millis(250);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);
const CAPTURE_BUDGET: Duration = Duration::from_secs(240);
const TOOL_CLICK_SLOP: f32 = 3.0;
const AUTOSAVE_IDLE: Duration = Duration::from_secs(8);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);
const EDITOR_SIZE: LogicalSize<f64> = LogicalSize::new(1440.0, 900.0);
const EDITOR_MINIMUM: LogicalSize<f64> = LogicalSize::new(1080.0, 700.0);

#[derive(Clone)]
enum Resume {
    Exit,
    Command(Command),
}

impl Resume {
    fn replaces_document(&self) -> bool {
        match self {
            Self::Exit => true,
            Self::Command(command) => command.replaces_document(),
        }
    }

    fn replaces_dataset(&self) -> bool {
        match self {
            Self::Exit => true,
            Self::Command(command) => command.replaces_dataset(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    Redraw,
}

pub struct App {
    surface: Option<Surface>,
    session: Session,
    wake: EventLoopProxy<Wake>,
    dialog: Option<Service>,
    awaiting: VecDeque<Awaiting>,
    engine: Option<Engine>,
    pending_engine: Option<Receiver<Engine>>,
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
    pump_at: Option<Instant>,
    title: Option<String>,
    modifiers: ModifiersState,
    output: Option<String>,
    fatal: Option<String>,
    ime: bool,
    ime_area: Option<(i32, i32, u32, u32)>,
    stream: Stream,
    pick_toggle: Option<bool>,
    mask_path: Vec<[f32; 3]>,
    plate: Option<(String, usize)>,
    export: Export,
    resume: Option<Resume>,
    release: Option<Release>,
    deferred: Option<Deferred>,
    dirty_released: bool,
    autosave_at: Option<Instant>,
    changed_at: Option<Instant>,
    revision: Revision,
    language: Language,
    camera_touched: bool,
    activity: Option<Instant>,
    animating: bool,
    chrome: window::Chrome,
    cursor: Option<winit::window::CursorIcon>,
    closing: bool,
    placement: Placement,
    annotation_placement: Option<(AnnotationKey, Vec<grana_figure::PlacedAnnotation>)>,
}

impl App {
    pub fn new(
        capture: Option<PathBuf>,
        startup: Option<String>,
        access: Access,
        wake: EventLoopProxy<Wake>,
    ) -> Self {
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
        let fit = if startup.is_some() {
            Fit::Editor
        } else {
            Fit::Startup
        };
        Self {
            surface: None,
            session,
            wake,
            dialog: None,
            awaiting: VecDeque::new(),
            engine: None,
            pending_engine: None,
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
            pump_at: None,
            title: None,
            modifiers: ModifiersState::default(),
            output: None,
            fatal: None,
            ime: false,
            ime_area: None,
            stream: Stream::new(),
            pick_toggle: None,
            mask_path: Vec::new(),
            plate: None,
            export: Export::new(),
            resume: None,
            release: None,
            deferred: None,
            dirty_released: false,
            autosave_at: None,
            changed_at: None,
            revision: Revision::ZERO,
            language,
            camera_touched: false,
            activity: None,
            animating: false,
            chrome: window::Chrome::new(),
            cursor: None,
            closing: false,
            placement: Placement::new(fit),
            annotation_placement: None,
        }
    }

    pub(super) fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let icon = winit::window::Icon::from_rgba(grana_brand::icon(256), 256, 256)
            .expect("the brand mark rasterizes to a window icon");
        let initial = if self.startup.is_some() {
            EDITOR_SIZE
        } else {
            LogicalSize::new(
                grana_editor::STARTUP_SIZE.x as f64,
                grana_editor::STARTUP_SIZE.y as f64,
            )
        };
        let window = Arc::new(
            event_loop
                .create_window(window::attributes(
                    Window::default_attributes()
                        .with_title("Grana")
                        .with_window_icon(Some(icon))
                        .with_visible(false)
                        .with_inner_size(initial),
                ))
                .expect("failed to create the window"),
        );
        let size = window.inner_size();
        window.set_ime_allowed(false);
        window.set_theme(Some(winit::window::Theme::Dark));
        if let Some(target) = self.startup.take() {
            let transfer = if grana_session::is_remote(&target) {
                self.session.begin_remote(target)
            } else {
                self.session.open_path(PathBuf::from(target))
            };
            if !matches!(transfer, grana_session::Transfer::Started) {
                grana_log::error!(target: "grana::app", "the startup target could not be opened");
            }
        }
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let wake = self.wake.clone();
        gpu.lost_handle().wake_with(move || {
            let _ = wake.send_event(Wake::Redraw);
        });
        let repaint = self.wake.clone();
        self.dialog = Some(Service::spawn(window.clone(), move || {
            let _ = repaint.send_event(Wake::Redraw);
        }));
        self.pending_engine = Some(spawn_engine(
            gpu.device.clone(),
            gpu.queue.clone(),
            gpu.format(),
            gpu.pipelines().clone(),
        ));
        let mut ui: Ui<grana_editor::Message> =
            Ui::new(&gpu.device, &gpu.queue, gpu.format(), gpu.pipelines());
        ui.set_clipboard(Box::new(SystemClipboard::new()));
        self.replaced = self.session.replaced();
        self.content = self.session.content();
        self.surface = Some(Surface::new(window, gpu, ui));
        self.cursor = None;
    }

    fn sync_engine(&mut self) {
        if self.engine.is_some() {
            return;
        }
        let Some(pending) = self.pending_engine.take() else {
            return;
        };
        let engine = match pending.try_recv() {
            Ok(engine) => engine,
            Err(TryRecvError::Empty) => {
                self.pending_engine = Some(pending);
                return;
            }
            Err(TryRecvError::Disconnected) => panic!("the renderer failed to initialize"),
        };
        self.engine = Some(engine);
        grana_log::info!(target: "grana::engine", "the render engine is ready");
        self.sync_dataset(false);
    }

    pub(super) fn pump(&mut self) -> bool {
        if !self.guard() {
            return false;
        }
        self.session.poll();
        self.poll_inquiry();
        self.sync_engine();
        if self.follow_resume() {
            return false;
        }
        if let Err(detail) = self.exchange_bricks() {
            self.fail(detail);
            return false;
        }
        self.poll_pick();
        self.session.sync_measure();
        true
    }

    pub(super) fn redraw(&mut self) {
        if self.fatal.is_some() {
            return;
        }
        let Some(size) = self.surface.as_mut().map(Surface::synchronize) else {
            return;
        };
        if !self.pump() {
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
        let (frame, wait) = self.run_ui(size);
        if let Some(choice) = frame.prompt {
            self.resolve_prompt(choice);
            if self.exiting {
                return;
            }
        }
        let animating = wait.is_some_and(|wait| wait.is_zero());
        self.place_window(&frame);
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
        let delivery = self.delivery();
        self.fit_view();
        if let Some(surface) = self.surface.as_ref()
            && !self.placement.settled(surface)
        {
            self.arm_pump();
            self.repaint_at = Some(Instant::now());
            return;
        }
        let mut delay = self.render(delivery).unwrap_or(Duration::MAX);
        self.reveal();
        if let Some(surface) = self.surface.as_ref()
            && let Some(wait) = self.placement.wait(surface)
        {
            delay = delay.min(wait);
        }
        if let Some(wait) = wait {
            delay = delay.min(wait);
        }
        if self.workspace.playback.playing {
            delay = delay.min(self.playback_delay());
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.session.scene().is_some() && self.settling() {
            delay = Duration::ZERO;
        }
        if let Some(active) = self.activity_wait() {
            delay = delay.min(active);
        }
        if !self.workspace.playback.playing && self.streaming() {
            delay = delay.min(self.stream_poll());
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
        let settled =
            loaded && streamed && info.converged && !info.fading && !self.session.previewing();
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
            self.repaint_at = Some(Instant::now());
        } else if delay < MAX_REPAINT_DELAY {
            self.repaint_at = Some(Instant::now() + delay);
        } else {
            self.repaint_at = None;
        }
        self.arm_pump();
    }

    pub(super) fn arm_pump(&mut self) {
        self.pump_at = (self.workspace.playback.playing && self.streaming())
            .then(|| Instant::now() + STREAM_POLL);
    }

    fn activity_wait(&self) -> Option<Duration> {
        self.activity
            .and_then(|at| INTERACTION_HOLD.checked_sub(at.elapsed()))
    }

    fn idle_delay(&self) -> Option<Duration> {
        if self.workspace.playback.playing {
            Some(self.playback_delay())
        } else if self.streaming() {
            Some(STREAM_POLL)
        } else if self.export.running() {
            Some(EXPORT_POLL)
        } else if self.capture.is_none()
            && self.deferred.is_none()
            && self.workspace.prompt.is_none()
            && self.session.access().author()
            && self.session.dirty()
        {
            Some(self.autosave_wait())
        } else {
            None
        }
    }
    pub(super) fn write_capture(&mut self, path: &Path) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let (pixels, size, format) = match surface.ui.capture() {
            Ok(captured) => captured,
            Err(detail) => {
                self.fail(detail);
                return;
            }
        };
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

    fn place_window(&mut self, frame: &Frame) {
        let Self {
            surface,
            placement,
            session,
            ..
        } = self;
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let fit = if session.scene().is_some()
            || (session.loading().is_some() && placement.fit() == Fit::Editor)
        {
            Fit::Editor
        } else {
            Fit::Startup
        };
        placement.place(fit, frame.startup, &surface.window);
    }

    fn reveal(&mut self) {
        let Self {
            surface,
            placement,
            capture,
            ..
        } = self;
        if let Some(surface) = surface.as_ref() {
            placement.reveal(surface, capture.is_some());
        }
    }

    pub(super) fn request_redraw(&self) {
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    pub(super) fn push_cursor(&mut self, cursor: winit::window::CursorIcon) {
        if self.cursor == Some(cursor) {
            return;
        }
        self.cursor = Some(cursor);
        if let Some(surface) = &self.surface {
            surface.window.set_cursor(cursor);
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

impl ApplicationHandler<Wake> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Wake) {
        match event {
            Wake::Redraw => self.request_redraw(),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            self.resume(event_loop);
        }
        if self.surface.is_some() {
            self.redraw();
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
            WindowEvent::Resized(_) => {
                if let Some(surface) = self.surface.as_ref() {
                    self.chrome.refresh(&surface.window);
                    if !self.camera_touched {
                        self.needs_fit = true;
                    }
                    surface.window.request_redraw();
                }
                self.repaint_at = Some(Instant::now());
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    self.chrome.refresh(&surface.window);
                    surface.window.request_redraw();
                }
                self.repaint_at = Some(Instant::now());
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
        let now = Instant::now();
        if self.pump_at.is_some_and(|at| now >= at) {
            self.pump_at = None;
            self.pump();
            if self.exiting {
                event_loop.exit();
                return;
            }
            self.arm_pump();
        }
        if self.repaint_at.is_some_and(|at| now >= at) {
            self.repaint_at = None;
            if self.placement.shown() {
                self.request_redraw();
            } else {
                self.redraw();
                if self.exiting {
                    event_loop.exit();
                    return;
                }
            }
        }
        if self.repaint_at.is_none() {
            let now = Instant::now();
            self.repaint_at = self.idle_delay().map(|delay| now + delay);
        }
        let deadline = match (self.repaint_at, self.pump_at) {
            (Some(repaint), Some(pump)) => Some(repaint.min(pump)),
            (repaint, pump) => repaint.or(pump),
        };
        match deadline {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(pending) = self.pending_engine.take() {
            let _ = pending.recv();
        }
    }
}

fn spawn_engine(
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipelines: grana_gpu::Pipelines,
) -> Receiver<Engine> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(Engine::new(&device, &queue, format, &pipelines));
    });
    receiver
}
