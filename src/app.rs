use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_render::{BrickRequest, FrameInfo, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::dataset::Dataset;
use crate::demo;
use crate::egui_layer::EguiLayer;
use crate::gpu::Gpu;
use crate::loader::{Event, Loader};
use crate::scene::Scene;
use crate::ui::{self, Message, UiState};
use crate::viewport::Viewport;

const TEXTURE_BUDGET: u64 = 512 << 20;
const UPLOAD_BATCH: usize = 32;
const MAX_IN_FLIGHT: usize = 64;
const STREAM_POLL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    egui: EguiLayer,
    renderer: Renderer,
}

pub struct App {
    surface: Option<Surface>,
    viewport: Viewport,
    scene: Option<Scene>,
    source: Option<Loader>,
    requests: Vec<BrickRequest>,
    in_flight: HashSet<BrickRequest>,
    frame: FrameInfo,
    loading: Option<String>,
    message: Option<Message>,
    repaint_at: Option<Instant>,
    needs_fit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface: None,
            viewport: Viewport::new(),
            scene: None,
            source: None,
            requests: Vec::new(),
            in_flight: HashSet::new(),
            frame: FrameInfo::default(),
            loading: None,
            message: None,
            repaint_at: None,
            needs_fit: true,
        }
    }

    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("grana")
                        .with_inner_size(LogicalSize::new(1440.0, 900.0)),
                )
                .expect("failed to create the window"),
        );
        let gpu = Gpu::new(window.clone());
        let egui = EguiLayer::new(&window, &gpu.device, gpu.format());
        let volume = demo::volume();
        let (source, pyramid) = Loader::memory(volume);
        let scene = Scene::new(Dataset::synthetic("synthetic fluorescence", pyramid));
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, gpu.format(), TEXTURE_BUDGET);
        renderer.set_pyramid(&gpu.device, scene.dataset.pyramid());
        self.scene = Some(scene);
        self.source = Some(source);
        self.surface = Some(Surface {
            window,
            gpu,
            egui,
            renderer,
        });
    }

    fn redraw(&mut self) {
        self.poll_source();
        if self.surface.is_none() || self.scene.is_none() {
            return;
        }
        let (output, actions) = self.run_ui();
        if actions.open
            && let Some(path) = Self::pick_folder()
        {
            self.begin_load(path);
        }
        if actions.fit {
            self.needs_fit = true;
        }
        let mut delay = self
            .render(output, actions.viewport)
            .unwrap_or(Duration::MAX);
        self.service_requests();
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if delay.is_zero() {
            self.repaint_at = None;
            if let Some(surface) = &self.surface {
                surface.window.request_redraw();
            }
        } else if delay < MAX_REPAINT_DELAY {
            self.repaint_at = Some(Instant::now() + delay);
        } else {
            self.repaint_at = None;
        }
    }

    fn streaming(&self) -> bool {
        self.loading.is_some()
            || !self.in_flight.is_empty()
            || (self.source.is_some() && !self.requests.is_empty())
            || (self.source.is_some() && self.frame.resident < self.frame.visible)
    }

    fn run_ui(&mut self) -> (egui::FullOutput, ui::Actions) {
        let surface = self.surface.as_mut().expect("the surface is alive");
        let scene = self.scene.as_mut().expect("the scene is alive");
        let Surface {
            window,
            gpu,
            egui,
            renderer: _,
        } = surface;
        let adapter = gpu.adapter_description.as_str();
        let loading = self.loading.as_deref();
        let message = self.message.as_ref();
        let frame = self.frame;
        let pending = self.in_flight.len();
        let mut actions = ui::Actions::new();
        let output = egui.run(window, |root| {
            actions = ui::build(
                root,
                &mut UiState {
                    scene,
                    adapter,
                    loading,
                    message,
                    frame,
                    pending,
                },
            );
        });
        (output, actions)
    }

    fn render(&mut self, output: egui::FullOutput, viewport_rect: egui::Rect) -> Option<Duration> {
        let surface = self.surface.as_mut()?;
        let scene = self.scene.as_mut()?;
        let Surface {
            window,
            gpu,
            egui,
            renderer,
        } = surface;
        let size = gpu.size();
        let region = physical_region(viewport_rect, window.scale_factor() as f32, size);
        let aspect = region[2] as f32 / region[3] as f32;
        self.viewport.resize(region[3] as f32);
        if self.needs_fit {
            self.viewport
                .fit(scene.dataset.world_size().to_array(), aspect);
            self.needs_fit = false;
        }
        let repaint_delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX);
        let params = scene.params();
        let frame = gpu.acquire()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.frame"),
            });
        self.requests.clear();
        self.frame = renderer.draw(
            &gpu.device,
            &gpu.queue,
            grana_render::Frame {
                encoder: &mut encoder,
                target: &view,
                region,
                camera: self.viewport.camera(),
                params: &params,
                requests: &mut self.requests,
            },
        );
        egui.paint(&gpu.device, &gpu.queue, &mut encoder, &view, size, output);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        Some(repaint_delay)
    }

    fn poll_source(&mut self) {
        let mut processed = 0;
        while processed < UPLOAD_BATCH {
            let Some(event) = self.source.as_ref().and_then(Loader::poll) else {
                break;
            };
            processed += 1;
            match event {
                Event::Opened { name, pyramid } => {
                    let dataset = Dataset::zarr(name, pyramid);
                    if let Some(surface) = self.surface.as_mut() {
                        surface
                            .renderer
                            .set_pyramid(&surface.gpu.device, dataset.pyramid());
                    }
                    self.scene = Some(Scene::new(dataset));
                    self.loading = None;
                    self.message = None;
                    self.in_flight.clear();
                    self.requests.clear();
                    self.needs_fit = true;
                }
                Event::Brick { request, data } => {
                    self.in_flight.remove(&request);
                    if let Some(surface) = self.surface.as_mut() {
                        surface.renderer.upload(
                            &surface.gpu.device,
                            &surface.gpu.queue,
                            request.level,
                            request.coord,
                            &data,
                        );
                    }
                }
                Event::ReadFailed { request, detail } => {
                    self.in_flight.remove(&request);
                    self.message = Some(Message::error(detail));
                    self.source = None;
                    self.in_flight.clear();
                    self.requests.clear();
                }
                Event::Failed(detail) => {
                    self.loading = None;
                    self.message = Some(Message::error(detail));
                    self.source = None;
                    self.in_flight.clear();
                    self.requests.clear();
                }
            }
        }
    }

    fn service_requests(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let mut stopped = false;
        for request in &self.requests {
            if self.in_flight.len() >= MAX_IN_FLIGHT {
                break;
            }
            if self.in_flight.contains(request) {
                continue;
            }
            if source.request(*request) {
                self.in_flight.insert(*request);
            } else {
                stopped = true;
                break;
            }
        }
        if stopped {
            self.source = None;
            self.in_flight.clear();
            self.message = Some(Message::error("the dataset reader stopped"));
        }
    }

    fn begin_load(&mut self, path: PathBuf) {
        if self.loading.is_some() {
            return;
        }
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file(path));
        self.message = None;
        self.in_flight.clear();
        self.requests.clear();
        self.repaint_at = Some(Instant::now());
    }

    fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr dataset")
            .pick_folder()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("OME-Zarr dataset")
        .to_string()
}

fn physical_region(rect: egui::Rect, scale: f32, surface: [u32; 2]) -> [u32; 4] {
    let x = ((rect.left() * scale).round() as i64).clamp(0, surface[0] as i64 - 1) as u32;
    let y = ((rect.top() * scale).round() as i64).clamp(0, surface[1] as i64 - 1) as u32;
    let width = ((rect.width() * scale).round() as u32)
        .min(surface[0] - x)
        .max(1);
    let height = ((rect.height() * scale).round() as u32)
        .min(surface[1] - y)
        .max(1);
    [x, y, width, height]
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
        let camera_changed = {
            let surface = self.surface.as_mut().expect("the surface is alive");
            let response = surface.egui.event(&surface.window, &event);
            self.viewport.on_window_event(&event, response.consumed)
        };
        if camera_changed && let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.gpu.resize(size);
                    surface.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if path.is_dir() {
                    self.begin_load(path);
                } else {
                    self.message = Some(Message::error(format!(
                        "{} is not an OME-Zarr folder",
                        path.display()
                    )));
                }
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.streaming() && self.repaint_at.is_none() {
            self.repaint_at = Some(Instant::now() + STREAM_POLL);
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
