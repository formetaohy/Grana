use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_render::Renderer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::dataset::{Dataset, LoadLimit, TEXTURE_BUDGET};
use crate::demo;
use crate::egui_layer::EguiLayer;
use crate::gpu::Gpu;
use crate::loader::Loader;
use crate::scene::Scene;
use crate::ui::{self, Message, UiState};
use crate::viewport::Viewport;

const POLL_INTERVAL: Duration = Duration::from_millis(80);
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
    loader: Option<Loader>,
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
            loader: None,
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
        let scene = Scene::new(Dataset::synthetic("synthetic fluorescence", &volume));
        let renderer = Renderer::new(&gpu.device, &gpu.queue, gpu.format(), &volume);
        self.scene = Some(scene);
        self.surface = Some(Surface {
            window,
            gpu,
            egui,
            renderer,
        });
    }

    fn redraw(&mut self) {
        if self.surface.is_none() || self.scene.is_none() {
            return;
        }
        self.poll_loader();
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
        if self.loader.is_some() {
            delay = delay.min(POLL_INTERVAL);
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
        let loading = self.loader.as_ref().map(Loader::name);
        let message = self.message.as_ref();
        let mut actions = ui::Actions::new();
        let output = egui.run(window, |root| {
            actions = ui::build(
                root,
                &mut UiState {
                    scene,
                    adapter,
                    loading,
                    message,
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
        renderer.draw(
            &gpu.queue,
            &mut encoder,
            &view,
            region,
            self.viewport.camera(),
            &params,
        );
        egui.paint(&gpu.device, &gpu.queue, &mut encoder, &view, size, output);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        Some(repaint_delay)
    }

    fn poll_loader(&mut self) {
        let Some(result) = self.loader.as_ref().and_then(Loader::poll) else {
            return;
        };
        self.loader = None;
        match result {
            Ok(loaded) => {
                let Some(surface) = self.surface.as_mut() else {
                    return;
                };
                surface.renderer.set_volume(
                    &surface.gpu.device,
                    &surface.gpu.queue,
                    &loaded.volume,
                );
                self.scene = Some(Scene::new(loaded.dataset));
                self.needs_fit = true;
                self.message = None;
            }
            Err(message) => self.message = Some(Message::error(message)),
        }
    }

    fn begin_load(&mut self, path: PathBuf) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        if self.loader.is_some() {
            return;
        }
        let limit = LoadLimit {
            max_dimension: surface.gpu.device.limits().max_texture_dimension_3d,
            budget: TEXTURE_BUDGET,
        };
        self.loader = Some(Loader::spawn(path, limit));
        self.message = None;
        self.repaint_at = Some(Instant::now() + POLL_INTERVAL);
    }

    fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr dataset")
            .pick_folder()
    }
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
        if self.loader.is_some() && self.repaint_at.is_none() {
            self.repaint_at = Some(Instant::now() + POLL_INTERVAL);
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
