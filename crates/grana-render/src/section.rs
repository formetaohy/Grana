use wgpu::{
    BindGroup, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::SCENE_FORMAT;

const SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/section.wgsl")
);

pub(crate) struct SectionPass {
    pipeline: RenderPipeline,
}

pub(crate) struct SectionFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
}

impl SectionPass {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.section"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.section"),
            layout: Some(&layouts.section),
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
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }

    pub fn draw(&self, encoder: &mut CommandEncoder, frame: SectionFrame<'_>) {
        let region = frame.region;
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.section.pass"),
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
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.draw(0..3, 0..1);
    }
}
