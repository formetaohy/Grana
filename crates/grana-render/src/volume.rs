use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState,
    ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, Device, FragmentState, LoadOp,
    MultisampleState, Operations, PipelineLayout, PrimitiveState, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StencilState, StoreOp, TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::{DEPTH_FORMAT, GUIDE_FORMAT, SCENE_FORMAT};

const RADIANCE: &str = concat!(
    "enable wgpu_binding_array;\n",
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
    include_str!("shader/composite.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl"),
    "\n",
    include_str!("shader/oit_key.wgsl"),
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
    include_str!("shader/composite.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl"),
    "\n",
    include_str!("shader/oit_key.wgsl"),
    include_str!("shader/oit_layers.wgsl"),
    "\n",
    include_str!("shader/ray.wgsl"),
    "\n",
    include_str!("shader/projection.wgsl")
);

const SURFACE: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/structure_tap.wgsl"),
    "\n",
    include_str!("shader/occupancy.wgsl"),
    "\n",
    include_str!("shader/ray.wgsl"),
    "\n",
    include_str!("shader/surface.wgsl")
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
    projection: Option<RenderPipeline>,
    surface: RenderPipeline,
}

pub(crate) struct VolumeFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub depth_group: &'a BindGroup,
    pub oit: &'a BindGroup,
    pub occupancy: &'a BindGroup,
    pub shadow: &'a BindGroup,
}

pub(crate) struct SurfaceFrame<'a> {
    pub normal: &'a TextureView,
    pub region: [u32; 4],
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub depth_group: &'a BindGroup,
    pub occupancy: &'a BindGroup,
}

impl VolumePass {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let composite = pipeline(
            device,
            &layouts.volume,
            "grana.volume",
            "fs_main",
            RADIANCE,
            [(SCENE_FORMAT, Some(BLEND))],
            pipelines,
        );
        let surface = pipeline(
            device,
            &layouts.volume_surface,
            "grana.volume.surface",
            "fs_surface",
            SURFACE,
            [(GUIDE_FORMAT, None)],
            pipelines,
        );
        Self {
            composite,
            projection: None,
            surface,
        }
    }

    pub fn draw(
        &mut self,
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        encoder: &mut CommandEncoder,
        frame: VolumeFrame<'_>,
        path: VolumePath,
    ) {
        let region = frame.region;
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
            depth_stencil_attachment: None,
            ..Default::default()
        });
        viewport(&mut pass, region);
        let pipeline = match path {
            VolumePath::Composite => &self.composite,
            VolumePath::Projection => self.projection.get_or_insert_with(|| {
                pipeline(
                    device,
                    &layouts.volume,
                    "grana.volume.projection",
                    "fs_main",
                    PROJECTION,
                    [(SCENE_FORMAT, Some(BLEND))],
                    pipelines,
                )
            }),
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.set_bind_group(3, frame.depth_group, &[]);
        pass.set_bind_group(4, frame.oit, &[]);
        pass.set_bind_group(5, frame.occupancy, &[]);
        pass.set_bind_group(6, frame.shadow, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn draw_surface(&self, encoder: &mut CommandEncoder, frame: SurfaceFrame<'_>) {
        let region = frame.region;
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.surface.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.normal,
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
        viewport(&mut pass, region);
        pass.set_pipeline(&self.surface);
        pass.set_bind_group(0, frame.scene, &[]);
        pass.set_bind_group(1, frame.level, &[]);
        pass.set_bind_group(2, frame.atlas, &[]);
        pass.set_bind_group(3, frame.depth_group, &[]);
        pass.set_bind_group(5, frame.occupancy, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn viewport(pass: &mut wgpu::RenderPass<'_>, region: [u32; 4]) {
    let width = region[2].max(1);
    let height = region[3].max(1);
    pass.set_viewport(
        region[0] as f32,
        region[1] as f32,
        width as f32,
        height as f32,
        0.0,
        1.0,
    );
    pass.set_scissor_rect(region[0], region[1], width, height);
}

fn pipeline(
    device: &Device,
    layout: &PipelineLayout,
    label: &str,
    fragment: &str,
    source: &'static str,
    targets: [(wgpu::TextureFormat, Option<BlendState>); 1],
    pipelines: &Pipelines,
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
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &targets.map(|(format, blend)| {
                Some(ColorTargetState {
                    format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })
            }),
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
