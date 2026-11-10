use wgpu::{
    BindGroup, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::SCENE_FORMAT;
use crate::volume::BLEND;

const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/composite.wgsl")
);

pub(crate) struct CompositePass {
    pipeline: RenderPipeline,
}

pub(crate) struct CompositeFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub depth: &'a BindGroup,
    pub layer: &'a BindGroup,
}

impl CompositePass {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.composite"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.composite"),
            layout: Some(&layouts.composite),
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
            cache: None,
        });
        Self { pipeline }
    }

    pub fn draw(&self, encoder: &mut CommandEncoder, frame: CompositeFrame<'_>) {
        let region = frame.region;
        assert!(
            region[2] > 0 && region[3] > 0,
            "a composite covers a positive pane"
        );
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.composite.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            region[2] as f32,
            region[3] as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], region[2], region[3]);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.depth, &[]);
        pass.set_bind_group(2, frame.layer, &[]);
        pass.draw(0..3, 0..1);
    }
}
