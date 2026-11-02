use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Buffer, BufferUsages, Color, ColorTargetState, ColorWrites,
    CommandEncoder, Device, FragmentState, LoadOp, Operations, PipelineLayoutDescriptor,
    PrimitiveState, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TextureFormat, TextureSampleType, TextureView, TextureViewDimension, VertexState,
};

const SHADER: &str = concat!(
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/present.wgsl")
);

const ACCUMULATE: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    flags: [f32; 4],
}

pub(crate) struct Present {
    accumulate: RenderPipeline,
    present: RenderPipeline,
    layout: BindGroupLayout,
    uniforms: Buffer,
    encode: bool,
    format: TextureFormat,
}

impl Present {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.present.layout"),
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
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sourced_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.present.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.present"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let accumulate = pipeline(
            device,
            "grana.present.accumulate",
            &shader,
            "fs_accumulate",
            crate::target::ACCUMULATION_FORMAT,
            &sourced_layout,
            Some(ACCUMULATE),
        );
        let present = pipeline(
            device,
            "grana.present",
            &shader,
            "fs_present",
            format,
            &sourced_layout,
            None,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.present.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            accumulate,
            present,
            layout,
            uniforms,
            encode: !format.is_srgb(),
            format,
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.format
    }

    pub fn bind(&self, device: &Device, source: &TextureView) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.present.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        })
    }

    pub fn write(&self, queue: &Queue, exposure: f32, samples: u32, transparent: bool) {
        assert!(
            exposure.is_finite() && exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        assert!(samples > 0, "a present pass resolves at least one sample");
        let uniforms = Uniforms {
            flags: [
                if self.encode { 1.0 } else { 0.0 },
                exposure,
                1.0 / samples as f32,
                if transparent { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn clear(&self, encoder: &mut CommandEncoder, target: &TextureView) {
        let _ = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.present.clear.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }

    pub fn accumulate(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        group: &BindGroup,
        reset: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.present.accumulate.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: if reset {
                        LoadOp::Clear(Color::TRANSPARENT)
                    } else {
                        LoadOp::Load
                    },
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.accumulate);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn present(&self, encoder: &mut CommandEncoder, target: &TextureView, group: &BindGroup) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.present.pass"),
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
    blend: Option<BlendState>,
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
                blend,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
