use grana_gpu::Pipelines;
use wgpu::{
    ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, Device,
    FragmentState, LoadOp, MultisampleState, Operations, PipelineLayoutDescriptor, PrimitiveState,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StencilState,
    StoreOp, TextureView, VertexState,
};

use crate::target::{ACCUMULATION_FORMAT, COUNT_FORMAT, DEPTH_FORMAT, SCENE_FORMAT};

const SHADER: &str = include_str!("shader/clear.wgsl");
const SIDE: u32 = 3;

pub(crate) struct Clear {
    scene: RenderPipeline,
    depth: RenderPipeline,
    residual: RenderPipeline,
    accumulation: RenderPipeline,
}

impl Clear {
    pub fn new(device: &Device, pipelines: &Pipelines) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.clear"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.clear.layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let scene = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.clear.scene"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Always),
                stencil: StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_color"),
                compilation_options: Default::default(),
                targets: &[Some(opaque(SCENE_FORMAT))],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let depth = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.clear.depth"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Always),
                stencil: StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let residual = pair(
            device,
            &module,
            &layout,
            "grana.clear.residual",
            crate::oit::RESIDUAL_COLOR_FORMAT,
            crate::oit::RESIDUAL_MOMENT_FORMAT,
            pipelines,
        );
        let accumulation = pair(
            device,
            &module,
            &layout,
            "grana.clear.accumulation",
            ACCUMULATION_FORMAT,
            COUNT_FORMAT,
            pipelines,
        );
        Self {
            scene,
            depth,
            residual,
            accumulation,
        }
    }

    pub fn scene(
        &self,
        encoder: &mut CommandEncoder,
        color: &TextureView,
        depth: &TextureView,
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.clear.scene.pass"),
            color_attachments: &[Some(attachment(color))],
            depth_stencil_attachment: Some(depth_attachment(depth)),
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(&self.scene);
        pass.draw(0..SIDE, 0..1);
    }

    pub fn depth(&self, encoder: &mut CommandEncoder, depth: &TextureView, region: [u32; 4]) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.clear.depth.pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(depth_attachment(depth)),
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(&self.depth);
        pass.draw(0..SIDE, 0..1);
    }

    pub fn residual(
        &self,
        encoder: &mut CommandEncoder,
        color: &TextureView,
        moment: &TextureView,
        region: [u32; 4],
    ) {
        self.pair_pass(
            encoder,
            "grana.clear.residual.pass",
            &self.residual,
            [color, moment],
            region,
        );
    }

    pub fn accumulation(
        &self,
        encoder: &mut CommandEncoder,
        accumulation: &TextureView,
        count: &TextureView,
        region: [u32; 4],
    ) {
        self.pair_pass(
            encoder,
            "grana.clear.accumulation.pass",
            &self.accumulation,
            [accumulation, count],
            region,
        );
    }

    fn pair_pass(
        &self,
        encoder: &mut CommandEncoder,
        label: &'static str,
        pipeline: &RenderPipeline,
        attachments: [&TextureView; 2],
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[
                Some(attachment(attachments[0])),
                Some(attachment(attachments[1])),
            ],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(pipeline);
        pass.draw(0..SIDE, 0..1);
    }
}

pub(crate) fn volume(
    encoder: &mut CommandEncoder,
    radiance: &TextureView,
    surface: &TextureView,
    normal: &TextureView,
) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.volume.clear.pass"),
        color_attachments: &[
            Some(cleared(radiance, wgpu::Color::TRANSPARENT)),
            Some(cleared(
                surface,
                wgpu::Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            )),
            Some(cleared(normal, wgpu::Color::TRANSPARENT)),
        ],
        depth_stencil_attachment: None,
        ..Default::default()
    });
}

fn cleared(view: &TextureView, color: wgpu::Color) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(color),
            store: StoreOp::Store,
        },
    }
}

fn opaque(format: wgpu::TextureFormat) -> ColorTargetState {
    ColorTargetState {
        format,
        blend: None,
        write_mask: ColorWrites::ALL,
    }
}

fn pair(
    device: &Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    label: &'static str,
    first: wgpu::TextureFormat,
    second: wgpu::TextureFormat,
    pipelines: &Pipelines,
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module,
            entry_point: Some("fs_pair"),
            compilation_options: Default::default(),
            targets: &[Some(opaque(first)), Some(opaque(second))],
        }),
        multiview_mask: None,
        cache: pipelines.cache(),
    })
}

fn attachment(view: &TextureView) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    }
}

fn depth_attachment(view: &TextureView) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    }
}
