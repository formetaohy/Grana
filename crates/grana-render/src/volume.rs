use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState,
    ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PipelineLayout, PrimitiveState, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StencilState, StoreOp, TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::{
    DEPTH_FORMAT, SCENE_FORMAT, VOLUME_FORMAT, VOLUME_NORMAL_FORMAT, VOLUME_SURFACE_FORMAT,
};

const MARCH: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/screen.wgsl"),
    "\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/brdf.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/structure_tap.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/optical.wgsl"),
    "\n",
    include_str!("shader/shade.wgsl"),
    "\n",
    include_str!("shader/occupancy.wgsl"),
    "\n",
    include_str!("shader/volume_walk.wgsl"),
    "\n",
    include_str!("shader/ray.wgsl"),
    "\n",
    include_str!("shader/march.wgsl")
);

const COMPOSITE: &str = concat!(
    include_str!("shader/screen.wgsl"),
    "\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/oit_key.wgsl"),
    "\n",
    include_str!("shader/oit_layers.wgsl"),
    "\n",
    include_str!("shader/volume.wgsl")
);

const SLICE: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/screen.wgsl"),
    "\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl")
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

pub(crate) struct VolumePass {
    march: RenderPipeline,
    composite: RenderPipeline,
    slice: RenderPipeline,
}

pub(crate) struct MarchFrame<'a> {
    pub radiance: &'a TextureView,
    pub surface: &'a TextureView,
    pub normal: &'a TextureView,
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub object: &'a BindGroup,
    pub occupancy: &'a BindGroup,
    pub shadow: &'a BindGroup,
}

pub(crate) struct CompositeFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub volume: &'a BindGroup,
    pub object: &'a BindGroup,
    pub oit: &'a BindGroup,
}

pub(crate) struct SliceFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
}

impl VolumePass {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let march = pipeline(
            device,
            &layouts.volume_march,
            "grana.volume.march",
            "fs_main",
            MARCH,
            &[
                (VOLUME_FORMAT, None),
                (VOLUME_SURFACE_FORMAT, None),
                (VOLUME_NORMAL_FORMAT, None),
            ],
            pipelines,
        );
        let composite = pipeline(
            device,
            &layouts.volume_composite,
            "grana.volume.composite",
            "fs_main",
            COMPOSITE,
            &[(SCENE_FORMAT, Some(BLEND))],
            pipelines,
        );
        let slice = pipeline(
            device,
            &layouts.volume_slice,
            "grana.volume.slice",
            "fs_main",
            SLICE,
            &[(SCENE_FORMAT, Some(BLEND))],
            pipelines,
        );
        Self {
            march,
            composite,
            slice,
        }
    }

    pub fn march(&self, encoder: &mut CommandEncoder, region: [u32; 4], frame: MarchFrame<'_>) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.march.pass"),
            color_attachments: &[
                Some(load(frame.radiance)),
                Some(load(frame.surface)),
                Some(load(frame.normal)),
            ],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        crate::target::viewport(
            &mut pass,
            [
                region[0] / 2,
                region[1] / 2,
                region[2].div_ceil(2),
                region[3].div_ceil(2),
            ],
        );
        pass.set_pipeline(&self.march);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.set_bind_group(3, frame.object, &[]);
        pass.set_bind_group(5, frame.occupancy, &[]);
        pass.set_bind_group(6, frame.shadow, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn composite(&self, encoder: &mut CommandEncoder, frame: CompositeFrame<'_>) {
        let mut pass = color_pass(
            encoder,
            "grana.volume.composite.pass",
            frame.target,
            frame.region,
        );
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.volume, &[]);
        pass.set_bind_group(2, frame.object, &[]);
        pass.set_bind_group(4, frame.oit, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn draw_slice(&self, encoder: &mut CommandEncoder, frame: SliceFrame<'_>) {
        let mut pass = color_pass(
            encoder,
            "grana.volume.slice.pass",
            frame.target,
            frame.region,
        );
        pass.set_pipeline(&self.slice);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn color_pass<'a>(
    encoder: &'a mut CommandEncoder,
    label: &'static str,
    target: &'a TextureView,
    region: [u32; 4],
) -> wgpu::RenderPass<'a> {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(load(target))],
        depth_stencil_attachment: None,
        ..Default::default()
    });
    crate::target::viewport(&mut pass, region);
    pass
}

fn load(view: &TextureView) -> RenderPassColorAttachment<'_> {
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

fn pipeline(
    device: &Device,
    layout: &PipelineLayout,
    label: &str,
    fragment: &str,
    source: &'static str,
    targets: &[(wgpu::TextureFormat, Option<BlendState>)],
    pipelines: &Pipelines,
) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    let states = targets
        .iter()
        .map(|(format, blend)| {
            Some(ColorTargetState {
                format: *format,
                blend: *blend,
                write_mask: ColorWrites::ALL,
            })
        })
        .collect::<Vec<_>>();
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
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &states,
        }),
        multiview_mask: None,
        cache: pipelines.cache(),
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
