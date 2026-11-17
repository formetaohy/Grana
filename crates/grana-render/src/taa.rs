use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2};
use grana_gpu::Pipelines;
use grana_gpu::layout::{sampler, texture, uniform};
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device, FilterMode, FragmentState,
    LoadOp, Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture, TextureUsages,
    TextureView, VertexState,
};

use crate::layout::Layouts;
use crate::target::{SCENE_FORMAT, image, view};

const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/geometry.wgsl"),
    "\n",
    include_str!("shader/object_surface.wgsl"),
    "\n",
    include_str!("shader/volume_surface.wgsl"),
    "\n",
    include_str!("shader/camera_surface.wgsl"),
    "\n",
    include_str!("shader/taa.wgsl")
);

const FEEDBACK: f32 = 1.0 / 16.0;

const MOTION_REACH: f32 = 8.0;

pub const SETTLE: u32 = 32;

pub(crate) const APERTURE_PHASE: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    previous: [[f32; 4]; 4],
    jitter: [f32; 4],
    flags: [f32; 4],
}

pub(crate) struct History {
    views: [TextureView; 2],
    resolved: Texture,
    resolved_view: TextureView,
    read: usize,
    intact: bool,
}

impl History {
    pub(crate) fn new(device: &Device, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a temporal history covers a positive surface"
        );
        let views = [
            view(&image(
                device,
                "grana.taa.history",
                size,
                SCENE_FORMAT,
                held(),
            )),
            view(&image(
                device,
                "grana.taa.history",
                size,
                SCENE_FORMAT,
                held(),
            )),
        ];
        let resolved = image(
            device,
            "grana.taa.resolved",
            size,
            SCENE_FORMAT,
            held() | TextureUsages::COPY_SRC,
        );
        let resolved_view = view(&resolved);
        Self {
            views,
            resolved,
            resolved_view,
            read: 0,
            intact: false,
        }
    }

    pub(crate) fn intact(&self) -> bool {
        self.intact
    }

    pub(crate) fn invalidate(&mut self) {
        self.intact = false;
    }

    pub(crate) fn read(&self) -> &TextureView {
        &self.views[self.read]
    }

    pub(crate) fn write(&self) -> &TextureView {
        &self.views[1 - self.read]
    }

    pub(crate) fn resolved(&self) -> &TextureView {
        &self.resolved_view
    }

    pub(crate) fn resolved_texture(&self) -> &Texture {
        &self.resolved
    }

    pub(crate) fn advance(&mut self) {
        self.read = 1 - self.read;
        self.intact = true;
    }
}

pub(crate) struct Resolve {
    pipeline: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: Buffer,
    sampler: Sampler,
}

impl Resolve {
    pub(crate) fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grana.taa.layout"),
            entries: &[
                uniform(0, ShaderStages::FRAGMENT),
                texture(1, ShaderStages::FRAGMENT),
                texture(2, ShaderStages::FRAGMENT),
                sampler(3, ShaderStages::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.taa.pipeline.layout"),
            bind_group_layouts: &[Some(&layouts.scene), Some(&layouts.geometry), Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.taa"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.taa.resolve"),
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
                ],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.taa.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.taa.sampler"),
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
        history: &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.taa.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(current),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(history),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    pub(crate) fn write(
        &self,
        queue: &Queue,
        previous: Mat4,
        jitter: [Vec2; 2],
        intact: bool,
        moved: bool,
    ) {
        let uniforms = Uniforms {
            previous: previous.to_cols_array_2d(),
            jitter: [jitter[0].x, jitter[0].y, jitter[1].x, jitter[1].y],
            flags: [
                f32::from(u8::from(intact)),
                FEEDBACK,
                MOTION_REACH,
                f32::from(u8::from(moved)),
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut CommandEncoder,
        groups: [&BindGroup; 3],
        retained: &TextureView,
        resolved: &TextureView,
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.taa.resolve.pass"),
            color_attachments: &[Some(loaded(retained)), Some(loaded(resolved))],
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(&self.pipeline);
        for (index, group) in groups.iter().enumerate() {
            pass.set_bind_group(index as u32, *group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
}

fn held() -> TextureUsages {
    TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING
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
