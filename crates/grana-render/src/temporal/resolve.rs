use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use grana_gpu::Pipelines;
use grana_gpu::layout::{sampler, texture, uniform};
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, Device, FilterMode, FragmentState, LoadOp,
    Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TextureSampleType, TextureView,
    TextureViewDimension, VertexState,
};

use super::{History, STATISTICS_FORMAT};
use crate::layout::Layouts;
use crate::target::{SCENE_FORMAT, viewport};

const SHADER: &str = concat!(
    include_str!("../shader/color.wgsl"),
    "\n",
    include_str!("../shader/scene.wgsl"),
    "\n",
    include_str!("../shader/octahedral.wgsl"),
    "\n",
    include_str!("../shader/geometry.wgsl"),
    "\n",
    include_str!("../shader/object_surface.wgsl"),
    "\n",
    include_str!("../shader/volume_surface.wgsl"),
    "\n",
    include_str!("../shader/camera_surface.wgsl"),
    "\n",
    include_str!("../shader/temporal.wgsl")
);

#[derive(Clone, Copy)]
pub(crate) struct State {
    pub(crate) previous: Mat4,
    pub(crate) eye: Vec3,
    pub(crate) intact: bool,
    pub(crate) clamp: bool,
    pub(crate) ceiling: f32,
    pub(crate) exposure: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    previous: [[f32; 4]; 4],
    eye: [f32; 4],
    flags: [f32; 4],
}

pub(crate) struct Resolve {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    uniforms: Buffer,
    sampler: Sampler,
}

impl Resolve {
    pub(crate) fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grana.temporal.layout"),
            entries: &[
                uniform(0, ShaderStages::FRAGMENT),
                texture(1, ShaderStages::FRAGMENT),
                texture(2, ShaderStages::FRAGMENT),
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler(4, ShaderStages::FRAGMENT),
                texture(5, ShaderStages::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.temporal.pipeline.layout"),
            bind_group_layouts: &[Some(&layouts.scene), Some(&layouts.geometry), Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.temporal"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.temporal.resolve"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    }),
                    Some(ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    }),
                    Some(ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    }),
                    Some(ColorTargetState {
                        format: STATISTICS_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.temporal.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.temporal.sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            uniforms,
            sampler,
        }
    }

    pub(crate) fn bind(
        &self,
        device: &Device,
        current: &TextureView,
        history: &History,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.temporal.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(current),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(history.read_color()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(history.read_statistics()),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(history.read_odd()),
                },
            ],
        })
    }

    pub(crate) fn write(&self, queue: &Queue, state: State) {
        let uniforms = Uniforms {
            previous: state.previous.to_cols_array_2d(),
            eye: state.eye.extend(0.0).to_array(),
            flags: [
                f32::from(u8::from(state.intact)),
                f32::from(u8::from(state.clamp)),
                state.ceiling,
                state.exposure,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut CommandEncoder,
        groups: [&BindGroup; 3],
        history: &History,
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.temporal.resolve.pass"),
            color_attachments: &[
                Some(loaded(history.write_color())),
                Some(loaded(history.resolved())),
                Some(loaded(history.write_odd())),
                Some(loaded(history.write_statistics())),
            ],
            ..Default::default()
        });
        viewport(&mut pass, region);
        pass.set_pipeline(&self.pipeline);
        for (index, group) in groups.iter().enumerate() {
            pass.set_bind_group(index as u32, *group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
}

fn loaded(view: &TextureView) -> RenderPassColorAttachment<'_> {
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
