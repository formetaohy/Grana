use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BufferDescriptor, BufferUsages, ColorTargetState,
    ColorWrites, Device, Extent3d, FragmentState, LoadOp, MultisampleState, Operations,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
    TextureViewDimension, VertexState,
};

use grana_render::{Image16, read_image16};

const FORMAT: TextureFormat = TextureFormat::Rgba32Float;

const SHADER: &str = include_str!("shader/compose.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ComposeData {
    flags: [f32; 4],
}

pub(crate) struct Composer {
    layout: BindGroupLayout,
    pipeline: RenderPipeline,
    uniforms: wgpu::Buffer,
    target: Option<(Texture, TextureView)>,
    size: [u32; 2],
}

impl Composer {
    pub(crate) fn new(device: &Device) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.compose.layout"),
            entries: &[
                texture(0),
                texture(1),
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.compose.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.compose"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.compose"),
            layout: Some(&pipeline_layout),
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
                entry_point: Some("fs_compose"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.compose.uniforms"),
            size: size_of::<ComposeData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline,
            uniforms,
            target: None,
            size: [0, 0],
        }
    }

    pub(crate) fn compose(
        &mut self,
        device: &Device,
        queue: &Queue,
        picture: &TextureView,
        overlay: &TextureView,
        size: [u32; 2],
        transparent: bool,
    ) -> Image16 {
        if self.target.is_none() || self.size != size {
            self.target = Some(build_target(device, size));
            self.size = size;
        }
        let (_, view) = self.target.as_ref().expect("the compose target is ready");
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&ComposeData {
                flags: [f32::from(u8::from(transparent)), 0.0, 0.0, 0.0],
            }),
        );
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.compose.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(picture),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(overlay),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.compose"),
        });
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.compose.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        queue.submit([encoder.finish()]);
        let (texture, _) = self.target.as_ref().expect("the compose target is ready");
        read_image16(device, queue, texture, size)
    }
}

fn texture(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn build_target(device: &Device, size: [u32; 2]) -> (Texture, TextureView) {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a composed picture covers at least one pixel"
    );
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.compose.target"),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}
