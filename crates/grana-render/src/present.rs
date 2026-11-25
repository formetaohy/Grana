use bytemuck::{Pod, Zeroable};
use grana_gpu::Pipelines;
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, Buffer,
    BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device,
    FilterMode, FragmentState, LoadOp, Operations, PipelineLayoutDescriptor, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TextureFormat, TextureSampleType, TextureView, TextureViewDimension, VertexState,
};

const SHADER: &str = concat!(
    include_str!("shader/color.wgsl"),
    "\n",
    include_str!("shader/present.wgsl")
);

const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    flags: [f32; 4],
    fade: [f32; 4],
    source: [f32; 4],
}

#[derive(Clone, Copy)]
struct Tone {
    encode: bool,
    grained: bool,
}

pub(crate) struct Present {
    present: RenderPipeline,
    quantize: RenderPipeline,
    layout: BindGroupLayout,
    uniforms: Buffer,
    sampler: Sampler,
    encode: bool,
    format: TextureFormat,
}

impl Present {
    pub fn new(device: &Device, format: TextureFormat, pipelines: &Pipelines) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.present.layout"),
            entries: &[
                texture(0),
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
                texture(2),
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.present.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.present"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let present = pipeline(
            device,
            "grana.present",
            &module,
            "fs_present",
            format,
            &pipeline_layout,
            pipelines,
        );
        let quantize = pipeline(
            device,
            "grana.present.quantize",
            &module,
            "fs_present",
            crate::target::WIDE_FORMAT,
            &pipeline_layout,
            pipelines,
        );
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.present.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.present.sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Self {
            present,
            quantize,
            layout,
            uniforms,
            sampler,
            encode: !format.is_srgb(),
            format,
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.format
    }

    pub fn bytes(&self) -> u64 {
        self.uniforms.size()
    }

    pub fn bind(&self, device: &Device, source: &TextureView, previous: &TextureView) -> BindGroup {
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
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(previous),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    pub fn write(
        &self,
        queue: &Queue,
        exposure: f32,
        transparent: bool,
        fade: Option<f32>,
        source: [f32; 4],
    ) {
        let tone = Tone {
            encode: self.encode,
            grained: true,
        };
        self.write_with(queue, tone, exposure, transparent, fade, source);
    }

    pub fn write_wide(&self, queue: &Queue, exposure: f32, transparent: bool) {
        let tone = Tone {
            encode: true,
            grained: false,
        };
        self.write_with(queue, tone, exposure, transparent, None, WHOLE);
    }

    fn write_with(
        &self,
        queue: &Queue,
        tone: Tone,
        exposure: f32,
        transparent: bool,
        fade: Option<f32>,
        source: [f32; 4],
    ) {
        assert!(
            exposure.is_finite() && exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        let weight = fade.unwrap_or(0.0);
        assert!(
            weight.is_finite() && (0.0..=1.0).contains(&weight),
            "a cross fade blends two resolved images"
        );
        let uniforms = Uniforms {
            flags: [
                if tone.encode { 1.0 } else { 0.0 },
                exposure,
                if transparent { 1.0 } else { 0.0 },
                if tone.grained { 1.0 } else { 0.0 },
            ],
            fade: [weight, 0.0, 0.0, 0.0],
            source,
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn present(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        group: &BindGroup,
        region: [u32; 4],
    ) {
        self.draw(encoder, target, &self.present, group, region);
    }

    pub fn quantize(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        group: &BindGroup,
        region: [u32; 4],
    ) {
        self.draw(encoder, target, &self.quantize, group, region);
    }

    fn draw(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        pipeline: &RenderPipeline,
        group: &BindGroup,
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.present.pass"),
            color_attachments: &[Some(load(target))],
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
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

fn texture(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn pipeline(
    device: &Device,
    label: &'static str,
    shader: &wgpu::ShaderModule,
    fragment: &str,
    format: TextureFormat,
    layout: &wgpu::PipelineLayout,
    pipelines: &Pipelines,
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
        cache: pipelines.cache(),
    })
}
