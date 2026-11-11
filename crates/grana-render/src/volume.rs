use wgpu::{
    BindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState,
    ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PipelineLayout, PrimitiveState, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StencilState, StoreOp, TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::{DEPTH_FORMAT, NORMAL_FORMAT, SCENE_FORMAT};

const COMPOSITE: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/composite.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl"),
    "\n",
    include_str!("shader/oit_layers.wgsl"),
    "\n",
    include_str!("shader/ray.wgsl"),
    "\n",
    include_str!("shader/volume.wgsl")
);

const PROJECTION: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/composite.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl"),
    "\n",
    include_str!("shader/oit_layers.wgsl"),
    "\n",
    include_str!("shader/ray.wgsl"),
    "\n",
    include_str!("shader/projection.wgsl")
);

pub(crate) const BLEND: BlendState = BlendState {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VolumePath {
    Composite,
    Projection,
}

pub(crate) struct VolumePass {
    composite: RenderPipeline,
    projection: RenderPipeline,
}

pub(crate) struct VolumeFrame<'a> {
    pub target: &'a TextureView,
    pub normal: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub depth_group: &'a BindGroup,
    pub oit: &'a BindGroup,
    pub occupancy: &'a BindGroup,
}

impl VolumePass {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let composite = pipeline(device, &layouts.volume, "grana.volume", COMPOSITE);
        let projection = pipeline(
            device,
            &layouts.volume,
            "grana.volume.projection",
            PROJECTION,
        );
        Self {
            composite,
            projection,
        }
    }

    pub fn draw(&self, encoder: &mut CommandEncoder, frame: VolumeFrame<'_>, path: VolumePath) {
        let region = frame.region;
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.pass"),
            color_attachments: &[
                Some(RenderPassColorAttachment {
                    view: frame.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                }),
                Some(RenderPassColorAttachment {
                    view: frame.normal,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                }),
            ],
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
        pass.set_pipeline(match path {
            VolumePath::Composite => &self.composite,
            VolumePath::Projection => &self.projection,
        });
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.set_bind_group(3, frame.depth_group, &[]);
        pass.set_bind_group(4, frame.oit, &[]);
        pass.set_bind_group(5, frame.occupancy, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn pipeline(
    device: &Device,
    layout: &PipelineLayout,
    label: &str,
    source: &'static str,
) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
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
            targets: &[
                Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BLEND),
                    write_mask: ColorWrites::ALL,
                }),
                Some(ColorTargetState {
                    format: NORMAL_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                }),
            ],
        }),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) fn depth_state(write: bool, compare: CompareFunction) -> DepthStencilState {
    DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}
