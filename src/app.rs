use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_render::{RenderParams, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::dataset::Dataset;
use crate::egui_layer::EguiLayer;
use crate::gpu::Gpu;
use crate::scene;
use crate::transfer_editor::TransferEditor;
use crate::ui::{self, UiState};
use crate::viewport::Viewport;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    egui: EguiLayer,
    renderer: Renderer,
}

pub struct App {
    surface: Option<Surface>,
    viewport: Viewport,
    transfer: TransferEditor,
    params: RenderParams,
    dataset: Option<Dataset>,
    repaint_at: Option<Instant>,
    needs_fit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface: None,
            viewport: Viewport::new(),
            transfer: TransferEditor::new(),
            params: RenderParams::default(),
            dataset: None,
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
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, gpu.format());

        let volume = scene::demo_volume();
        renderer.set_volume(&gpu.device, &gpu.queue, &volume);
        renderer.set_transfer_function(&gpu.queue, &self.transfer.transfer());

        self.dataset = Some(Dataset::new("synthetic fluorescence", &volume));
        self.surface = Some(Surface {
            window,
            gpu,
            egui,
            renderer,
        });
    }

    fn redraw(&mut self) {
        let App {
            surface,
            viewport,
            transfer,
            params,
            dataset,
            repaint_at,
            needs_fit,
        } = self;
        let (Some(surface), Some(dataset)) = (surface.as_mut(), dataset.as_ref()) else {
            return;
        };
        let Surface {
            window,
            gpu,
            egui,
            renderer,
        } = surface;

        let size = gpu.size();

        let mut actions = ui::Actions::new();
        let output = egui.run(window, |root| {
            let mut state = UiState {
                transfer,
                params,
                dataset,
                adapter: &gpu.adapter_description,
            };
            actions = ui::build(root, &mut state);
        });
        if transfer.take_dirty() {
            renderer.set_transfer_function(&gpu.queue, &transfer.transfer());
        }

        let region = physical_region(actions.viewport, window.scale_factor() as f32, size);
        let aspect = region[2] as f32 / region[3] as f32;
        viewport.resize(region[3] as f32);
        if actions.fit || *needs_fit {
            viewport.fit(dataset.world_size.to_array(), aspect);
            *needs_fit = false;
        }

        let Some(frame) = gpu.acquire() else {
            return;
        };
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
            viewport.camera(),
            params,
        );
        let repaint_delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX);
        egui.paint(&gpu.device, &gpu.queue, &mut encoder, &view, size, output);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);

        if repaint_delay.is_zero() {
            window.request_redraw();
        } else if repaint_delay < Duration::from_secs(3600) {
            *repaint_at = Some(Instant::now() + repaint_delay);
        }
    }
}

fn physical_region(rect: egui::Rect, scale: f32, surface: [u32; 2]) -> [u32; 4] {
    let x = ((rect.left() * scale).round() as i64).clamp(0, surface[0] as i64) as u32;
    let y = ((rect.top() * scale).round() as i64).clamp(0, surface[1] as i64) as u32;
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
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Surface {
            window,
            gpu,
            egui,
            renderer: _,
        } = surface;

        let response = egui.event(window, &event);
        let camera_changed = self.viewport.on_window_event(&event, response.consumed);
        if camera_changed {
            window.request_redraw();
        }

        let redraw_requested = matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gpu.resize(size);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            _ => {}
        }
        if redraw_requested {
            self.redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
