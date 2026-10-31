use egui::{Pos2, Rect};
use wgpu::{CommandEncoder, Device, Queue, TextureView};

use crate::overlay::{self, Legend};

pub struct Target<'a> {
    pub texture: &'a TextureView,
    pub size: [u32; 2],
}

pub struct Caption<'a> {
    pub world_per_point: f32,
    pub pixels_per_point: f32,
    pub legends: &'a [Legend],
}

pub struct Figure {
    context: egui::Context,
    renderer: egui_wgpu::Renderer,
}

impl Figure {
    pub fn new(device: &Device, format: wgpu::TextureFormat) -> Self {
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        Self {
            context,
            renderer: egui_wgpu::Renderer::new(
                device,
                format,
                egui_wgpu::RendererOptions::default(),
            ),
        }
    }

    pub fn paint(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: Target<'_>,
        caption: Caption<'_>,
    ) {
        let Target { texture, size } = target;
        let Caption {
            world_per_point,
            pixels_per_point,
            legends,
        } = caption;
        assert!(
            size[0] > 0 && size[1] > 0,
            "a figure covers at least one pixel"
        );
        assert!(
            world_per_point.is_finite() && world_per_point > 0.0,
            "a figure maps a positive world span onto a point"
        );
        assert!(
            pixels_per_point.is_finite() && pixels_per_point >= 1.0,
            "a figure resolves at least one pixel per point"
        );
        self.context.set_pixels_per_point(pixels_per_point);
        let points = egui::vec2(
            size[0] as f32 / pixels_per_point,
            size[1] as f32 / pixels_per_point,
        );
        let screen = Rect::from_min_size(Pos2::ZERO, points);
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let output = self.context.run_ui(input, |ui| {
            overlay::figure(ui.painter(), screen, world_per_point, legends);
        });
        let egui::FullOutput {
            mut textures_delta,
            shapes,
            ..
        } = output;
        let shapes = self.context.tessellate(shapes, pixels_per_point);
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point,
        };
        self.renderer
            .update_buffers(device, queue, encoder, &shapes, &descriptor);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grana.figure.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: texture,
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
            self.renderer.render(&mut pass, &shapes, &descriptor);
        }
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        textures_delta.clear();
    }
}
