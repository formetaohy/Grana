use wgpu::{
    BindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState,
    ColorWrites, CommandEncoder, DepthStencilState, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StencilState,
    StoreOp, TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::{DEPTH_FORMAT, SCENE_FORMAT};

const SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/volume.wgsl")
);

const BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

pub(crate) struct VolumePass {
    pipeline: RenderPipeline,
}

pub(crate) struct VolumeFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub fallback: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub depth_group: &'a BindGroup,
    pub shadow: &'a BindGroup,
    pub oit: &'a BindGroup,
}

impl VolumePass {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.volume"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.volume"),
            layout: Some(&layouts.volume),
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

    pub fn draw(&self, encoder: &mut CommandEncoder, frame: VolumeFrame<'_>) {
        let region = frame.region;
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.pass"),
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
        pass.set_bind_group(3, frame.depth_group, &[]);
        pass.set_bind_group(4, frame.shadow, &[]);
        pass.set_bind_group(5, frame.oit, &[]);
        pass.set_bind_group(6, frame.fallback, &[]);
        pass.draw(0..3, 0..1);
    }
}

pub(crate) fn depth_state(write: bool, compare: wgpu::CompareFunction) -> DepthStencilState {
    DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}
