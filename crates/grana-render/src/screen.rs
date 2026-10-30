use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType, BufferUsages,
    Color, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, TextureFormat, TextureSampleType, TextureView,
    TextureViewDimension, VertexState,
};

use crate::target::SCENE_FORMAT;

const SHADER: &str = concat!(
    include_str!("color.wgsl"),
    "\n",
    include_str!("screen.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    flags: [f32; 4],
}

pub(crate) struct Screen {
    background: RenderPipeline,
    present: RenderPipeline,
    layout: BindGroupLayout,
    uniforms: Buffer,
    encode: bool,
}

impl Screen {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.screen.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.screen.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let background_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.screen.background_layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.screen"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let background = pipeline(
            device,
            "grana.screen.background",
            &shader,
            "fs_background",
            SCENE_FORMAT,
            &background_layout,
        );
        let present = pipeline(
            device,
            "grana.screen.present",
            &shader,
            "fs_present",
            format,
            &pipeline_layout,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.screen.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            background,
            present,
            layout,
            uniforms,
            encode: !format.is_srgb(),
        }
    }

    pub fn bind(&self, device: &Device, color: &TextureView) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.screen.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(color),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        })
    }

    pub fn write(&self, queue: &Queue) {
        let uniforms = Uniforms {
            flags: [if self.encode { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn background(&self, encoder: &mut CommandEncoder, target: &TextureView, region: [u32; 4]) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.screen.background.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            region[2].max(1) as f32,
            region[3].max(1) as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], region[2].max(1), region[3].max(1));
        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);
    }

    pub fn present(&self, encoder: &mut CommandEncoder, target: &TextureView, group: &BindGroup) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.screen.present.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.present);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn pipeline(
    device: &Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    fragment: &str,
    format: TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
