use grana_gpu::layout::{sampler, texture};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, ColorTargetState, ColorWrites, CommandEncoder,
    Device, Extent3d, FragmentState, LoadOp, MultisampleState, Operations,
    PipelineLayoutDescriptor, PrimitiveState, RenderPass, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
    ShaderStages, StoreOp, TextureDescriptor, TextureDimension, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

use crate::layout::Layouts;
use crate::target::{CUT_ID_FORMAT, SCENE_FORMAT, STENCIL_FORMAT};

const CUT_SAMPLES: u32 = 4;

const SHADER: &str = include_str!("shader/cut.wgsl");

pub(crate) struct CutLayer {
    size: [u32; 2],
    composite_layout: BindGroupLayout,
    outline_layout: BindGroupLayout,
    sampler: Sampler,
    composite: RenderPipeline,
    outline: RenderPipeline,
    target: Option<Target>,
    targets: u64,
}

struct Target {
    color: TextureView,
    resolved: TextureView,
    stencil: TextureView,
    identities: TextureView,
    group: BindGroup,
    identities_group: BindGroup,
}

impl CutLayer {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let composite_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.cut.layout"),
            entries: &[
                texture(0, ShaderStages::FRAGMENT),
                sampler(1, ShaderStages::FRAGMENT),
            ],
        });
        let outline_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.cut.outline.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.cut.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.cut"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let composite_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.cut.pipeline_layout"),
            bind_group_layouts: &[Some(&composite_layout)],
            immediate_size: 0,
        });
        let composite = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.cut"),
            layout: Some(&composite_pipeline_layout),
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
                entry_point: Some("fs_composite"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let outline_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.cut.outline.pipeline_layout"),
            bind_group_layouts: &[Some(&outline_layout), Some(&layouts.cut)],
            immediate_size: 0,
        });
        let outline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.cut.outline"),
            layout: Some(&outline_pipeline_layout),
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
                entry_point: Some("fs_outline"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            size: [0, 0],
            composite_layout,
            outline_layout,
            sampler,
            composite,
            outline,
            target: None,
            targets: 0,
        }
    }

    pub fn targets(&self) -> u64 {
        self.targets
    }

    pub fn draw<'a, T>(
        &mut self,
        device: &Device,
        encoder: &'a mut CommandEncoder,
        size: [u32; 2],
        body: impl FnOnce(&mut RenderPass<'a>) -> T,
    ) -> T {
        if self.target.is_none() || self.size != size {
            self.target = Some(self.build_target(device, size));
            self.size = size;
            self.targets += 1;
        }
        let target = self.target.as_ref().expect("the cut target is ready");
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.cut.pass"),
            color_attachments: &[
                Some(RenderPassColorAttachment {
                    view: &target.color,
                    depth_slice: None,
                    resolve_target: Some(&target.resolved),
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Discard,
                    },
                }),
                Some(RenderPassColorAttachment {
                    view: &target.identities,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &target.stencil,
                depth_ops: None,
                stencil_ops: Some(Operations {
                    load: LoadOp::Clear(0),
                    store: StoreOp::Discard,
                }),
            }),
            ..Default::default()
        });
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, size[0], size[1]);
        body(&mut pass)
    }

    pub fn outline(&self, encoder: &mut CommandEncoder, size: [u32; 2], data: &BindGroup) {
        let Some(target) = self.target.as_ref() else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.cut.outline.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &target.resolved,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, size[0], size[1]);
        pass.set_pipeline(&self.outline);
        pass.set_bind_group(0, &target.identities_group, &[]);
        pass.set_bind_group(1, data, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn composite(&self, encoder: &mut CommandEncoder, target: &TextureView, region: [u32; 4]) {
        let Some(layer) = self.target.as_ref() else {
            return;
        };
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.cut.composite.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
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
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &layer.group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn build_target(&self, device: &Device, size: [u32; 2]) -> Target {
        assert!(
            size[0] > 0 && size[1] > 0,
            "the cut layer covers a non empty pane"
        );
        let extent = Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let color = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.cut.color"),
                size: extent,
                mip_level_count: 1,
                sample_count: CUT_SAMPLES,
                dimension: TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let resolved = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.cut.resolved"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let stencil = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.cut.stencil"),
                size: extent,
                mip_level_count: 1,
                sample_count: CUT_SAMPLES,
                dimension: TextureDimension::D2,
                format: STENCIL_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let identities = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.cut.identities"),
                size: extent,
                mip_level_count: 1,
                sample_count: CUT_SAMPLES,
                dimension: TextureDimension::D2,
                format: CUT_ID_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.cut.group"),
            layout: &self.composite_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolved),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let identities_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.cut.identities.group"),
            layout: &self.outline_layout,
            entries: &[BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&identities),
            }],
        });
        Target {
            color,
            resolved,
            stencil,
            identities,
            group,
            identities_group,
        }
    }
}

pub(crate) fn multisample() -> MultisampleState {
    MultisampleState {
        count: CUT_SAMPLES,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

pub(crate) fn pass_state() -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Always,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0x00,
            write_mask: 0x00,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}
