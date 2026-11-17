use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, ColorTargetState, ColorWrites,
    CommandEncoder, Device, Extent3d, FragmentState, LoadOp, MultisampleState, Operations,
    PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, VertexState,
};

use crate::layout::Layouts;

const OBJECTS: &str = concat!(
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/geometry.wgsl"),
    "\n",
    include_str!("shader/object_surface.wgsl"),
    "\n",
    include_str!("shader/surface_common.wgsl"),
    "\n",
    include_str!("shader/surface_objects.wgsl")
);

const MERGED: &str = concat!(
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/geometry.wgsl"),
    "\n",
    include_str!("shader/object_surface.wgsl"),
    "\n",
    include_str!("shader/volume_surface.wgsl"),
    "\n",
    include_str!("shader/surface_common.wgsl"),
    "\n",
    include_str!("shader/surface_merged.wgsl")
);

pub(crate) const FORMAT: TextureFormat = TextureFormat::Rgba32Float;

const SIDE: u32 = 3;

pub(crate) enum Source<'a> {
    Objects(&'a BindGroup),
    Merged(&'a BindGroup),
}

pub(crate) struct Target {
    _texture: Texture,
    view: TextureView,
    read: BindGroup,
}

impl Target {
    pub fn new(device: &Device, layouts: &Layouts, size: [u32; 2]) -> Self {
        let extent = [size[0].div_ceil(2), size[1].div_ceil(2)];
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.scene.surface"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let read = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.scene.surface.group"),
            layout: &layouts.surface,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            }],
        });
        Self {
            _texture: texture,
            view,
            read,
        }
    }

    pub fn read(&self) -> &BindGroup {
        &self.read
    }
}

pub(crate) struct Pipeline {
    objects: RenderPipeline,
    merged: RenderPipeline,
}

impl Pipeline {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let objects = pipeline(
            device,
            &layouts.surface_objects,
            "grana.scene.surface.objects",
            OBJECTS,
            pipelines,
        );
        let merged = pipeline(
            device,
            &layouts.surface_merged,
            "grana.scene.surface.merged",
            MERGED,
            pipelines,
        );
        Self { objects, merged }
    }

    pub fn resolve(
        &self,
        encoder: &mut CommandEncoder,
        target: &Target,
        region: [u32; 4],
        source: Source<'_>,
    ) {
        let (pipeline, group) = match source {
            Source::Objects(group) => (&self.objects, group),
            Source::Merged(group) => (&self.merged, group),
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.scene.surface.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &target.view,
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
        crate::target::viewport(
            &mut pass,
            [
                region[0] / 2,
                region[1] / 2,
                region[2].div_ceil(2),
                region[3].div_ceil(2),
            ],
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..SIDE, 0..1);
    }
}

fn pipeline(
    device: &Device,
    layout: &wgpu::PipelineLayout,
    label: &str,
    source: &str,
    pipelines: &Pipelines,
) -> RenderPipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: pipelines.cache(),
    })
}
