use egui::ViewportId;
use winit::window::Window;

pub struct EguiLayer {
    context: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl EguiLayer {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        context.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.slider_width = 120.0;
        });

        let state = egui_winit::State::new(
            context.clone(),
            ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());

        Self {
            context,
            state,
            renderer,
        }
    }

    pub fn event(
        &mut self,
        window: &Window,
        event: &winit::event::WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    pub fn run(&mut self, window: &Window, build: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let input = self.state.take_egui_input(window);
        let output = self.context.run_ui(input, build);
        self.state
            .handle_platform_output(window, output.platform_output.clone());
        output
    }

    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        output: egui::FullOutput,
    ) {
        let egui::FullOutput {
            mut textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = output;
        self.upload(device, queue, &mut textures_delta);
        let shapes = self.context.tessellate(shapes, pixels_per_point);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point,
        };
        self.renderer
            .update_buffers(device, queue, encoder, &shapes, &screen_descriptor);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grana.egui.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &shapes, &screen_descriptor);
        }

        self.release(&mut textures_delta);
    }

    pub fn discard(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output: egui::FullOutput,
    ) {
        let mut textures_delta = output.textures_delta;
        self.upload(device, queue, &mut textures_delta);
        self.release(&mut textures_delta);
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures_delta: &mut egui::TexturesDelta,
    ) {
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        textures_delta.set.clear();
    }

    fn release(&mut self, textures_delta: &mut egui::TexturesDelta) {
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        textures_delta.clear();
    }
}
