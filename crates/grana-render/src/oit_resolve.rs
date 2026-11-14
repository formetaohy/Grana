use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::SCENE_FORMAT;
use crate::volume::BLEND;

const RESOLVE: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/oit_layers.wgsl"),
    "\n",
    include_str!("shader/oit_resolve.wgsl")
);

pub(crate) struct OitResolve {
    pipeline: RenderPipeline,
}

impl OitResolve {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.oit.resolve"),
            source: ShaderSource::Wgsl(RESOLVE.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.oit.resolve"),
            layout: Some(&layouts.oit_resolve),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BLEND),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        Self { pipeline }
    }

    pub fn draw(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        region: [u32; 4],
        scene: &BindGroup,
        oit: &BindGroup,
    ) {
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.oit.resolve.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.draw(0..3, 0..1);
    }
}
