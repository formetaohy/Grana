use std::num::NonZeroU32;

use bytemuck::cast_slice;
use grana_volume::Pyramid;
use half::f16;
use wgpu::{
    BindGroup, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer, BufferBindingType,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState,
    MultisampleState, Origin3d, PipelineLayoutDescriptor, PrimitiveState, Queue,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, TexelCopyBufferLayout,
    TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
    TextureViewDimension, VertexState,
};

use crate::cache::Cache;
use crate::level::Resources;
use crate::marker::Markers;
use crate::mesh::Surfaces;
use crate::screen::Screen;
use crate::target::{SCENE_FORMAT, SceneTarget, depth_layout};
use crate::transfer::TransferFunction;
use crate::uniforms;
use crate::{ChannelParams, MAX_CHANNEL};

const LUT_SIZE: u32 = 1024;
const DUMMY_VOXEL: [u16; 1] = [0];

const VOLUME_SHADER: &str = concat!(
    include_str!("sampling.wgsl"),
    "\n",
    include_str!("volume.wgsl")
);
const SECTION_SHADER: &str = concat!(
    include_str!("sampling.wgsl"),
    "\n",
    include_str!("color.wgsl"),
    "\n",
    include_str!("section.wgsl")
);

const VOLUME_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::SrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::Zero,
        operation: BlendOperation::Add,
    },
};

const _: () = assert!(
    grana_volume::BRICK_SIDE == 32 && grana_volume::PADDED_SIDE == 34,
    "the volume shader bakes the brick dimensions"
);

pub(crate) struct Backend {
    pub volume_pipeline: RenderPipeline,
    pub section_pipeline: RenderPipeline,
    pub markers: Markers,
    pub surfaces: Surfaces,
    pub screen: Screen,
    pub uniforms: Buffer,
    pub section_uniforms: Buffer,
    depth_layout: BindGroupLayout,
    target: Option<SceneTarget>,
    present_group: Option<BindGroup>,
    frame_layout: BindGroupLayout,
    channel_layout: BindGroupLayout,
    lut: Texture,
    lut_view: TextureView,
    lut_sampler: Sampler,
    volume_sampler: Sampler,
    dummy_view: TextureView,
}

impl Backend {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let frame_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.frame.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let channel_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.channel.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: NonZeroU32::new(MAX_CHANNEL as u32),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let depth_layout = depth_layout(device);
        let volume_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.volume.pipeline_layout"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&channel_layout),
                Some(&depth_layout),
            ],
            immediate_size: 0,
        });
        let section_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.section.pipeline_layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&channel_layout)],
            immediate_size: 0,
        });
        let volume_shader = shader(device, "grana.volume", VOLUME_SHADER);
        let section_shader = shader(device, "grana.section", SECTION_SHADER);
        let volume_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.volume"),
            layout: Some(&volume_layout),
            vertex: VertexState {
                module: &volume_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(VOLUME_BLEND),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let section_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.section"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_shader,
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
        let markers = Markers::new(device, &frame_layout);
        let surfaces = Surfaces::new(device, &frame_layout);
        let screen = Screen::new(device, format);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.volume.uniforms"),
            size: size_of::<uniforms::Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let section_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.section.uniforms"),
            size: size_of::<uniforms::SectionUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let volume_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.atlas.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let lut = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume.lut"),
            size: wgpu::Extent3d {
                width: LUT_SIZE,
                height: MAX_CHANNEL as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.volume.lut.view"),
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.volume.lut.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy = device.create_texture(&TextureDescriptor {
            label: Some("grana.atlas.dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: TextureFormat::R16Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &dummy,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            cast_slice(&DUMMY_VOXEL),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(2),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy.create_view(&TextureViewDescriptor {
            label: Some("grana.atlas.dummy.view"),
            ..Default::default()
        });
        Self {
            volume_pipeline,
            section_pipeline,
            markers,
            surfaces,
            screen,
            uniforms,
            section_uniforms,
            depth_layout,
            target: None,
            present_group: None,
            frame_layout,
            channel_layout,
            lut,
            lut_view,
            lut_sampler,
            volume_sampler,
            dummy_view,
        }
    }

    pub fn cache(&self, device: &Device, pyramid: &Pyramid, budget: u64) -> Cache {
        Cache::new(
            device,
            pyramid,
            &self.channel_layout,
            &self.dummy_view,
            &self.volume_sampler,
            budget,
        )
    }

    pub fn resources(&self) -> Resources<'_> {
        Resources {
            layout: &self.frame_layout,
            uniforms: &self.uniforms,
            section_uniforms: &self.section_uniforms,
            lut: &self.lut_view,
            lut_sampler: &self.lut_sampler,
        }
    }

    pub fn ensure_target(&mut self, device: &Device, surface: [u32; 2]) {
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.size() == surface)
        {
            return;
        }
        let target = SceneTarget::new(device, &self.depth_layout, surface);
        self.present_group = Some(self.screen.bind(device, target.color()));
        self.target = Some(target);
    }

    pub fn scene(&self) -> &TextureView {
        self.target
            .as_ref()
            .expect("the scene target is ready")
            .color()
    }

    pub fn depth(&self) -> &TextureView {
        self.target
            .as_ref()
            .expect("the scene target is ready")
            .depth()
    }

    pub fn depth_group(&self) -> &BindGroup {
        self.target
            .as_ref()
            .expect("the scene target is ready")
            .depth_group()
    }

    pub fn clear_depth(&self, encoder: &mut CommandEncoder) {
        let _ = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.depth.clear"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: self.depth(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
    }

    pub fn present(&self) -> &BindGroup {
        self.present_group
            .as_ref()
            .expect("the present group is ready")
    }

    pub fn write_transfers(
        &self,
        queue: &Queue,
        transfers: &mut [Option<TransferFunction>],
        channels: &[ChannelParams],
    ) {
        for (index, channel) in channels.iter().enumerate() {
            if transfers[index].as_ref() == Some(&channel.transfer) {
                continue;
            }
            self.write_transfer(queue, index, &channel.transfer);
            transfers[index] = Some(channel.transfer.clone());
        }
    }

    fn write_transfer(&self, queue: &Queue, index: usize, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE as usize);
        let mut quantized = Vec::with_capacity(lut.len() * 4);
        for [red, green, blue, opacity] in lut {
            for channel in [red, green, blue, opacity] {
                quantized.push(f16::from_f32(channel).to_bits());
            }
        }
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.lut,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: index as u32,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            cast_slice(&quantized),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LUT_SIZE * 8),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: LUT_SIZE,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn shader(device: &Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    })
}
