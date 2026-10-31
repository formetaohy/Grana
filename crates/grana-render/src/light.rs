use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, CommandEncoder,
    ComputePipeline, ComputePipelineDescriptor, Device, Extent3d, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension,
};

use crate::layout::Layouts;

const SIDE: u32 = 48;
const MIN_SIDE: u32 = 4;
const WORKGROUP: u32 = 4;
const SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/light.wgsl")
);

pub(crate) struct Light {
    sample: BindGroup,
    write: BindGroup,
    pipeline: ComputePipeline,
    dims: [u32; 3],
}

impl Light {
    pub fn new(device: &Device, layouts: &Layouts, world: [f32; 3]) -> Self {
        let dims = resolution(world);
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.light"),
            size: Extent3d {
                width: dims[0],
                height: dims[1],
                depth_or_array_layers: dims[2],
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("grana.light.view"),
            dimension: Some(TextureViewDimension::D3),
            ..Default::default()
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.light.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sample = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.light.group"),
            layout: &layouts.light,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });
        let write = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.light.write.group"),
            layout: &layouts.light_write,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            }],
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.light"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.light"),
            layout: Some(&layouts.light_pipeline),
            module: &module,
            entry_point: Some("light_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            sample,
            write,
            pipeline,
            dims,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.sample
    }

    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        scene: &BindGroup,
        level: &BindGroup,
        atlas: &BindGroup,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.light.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, level, &[]);
        pass.set_bind_group(2, atlas, &[]);
        pass.set_bind_group(3, &self.write, &[]);
        pass.dispatch_workgroups(
            self.dims[0].div_ceil(WORKGROUP),
            self.dims[1].div_ceil(WORKGROUP),
            self.dims[2].div_ceil(WORKGROUP),
        );
    }
}

fn resolution(world: [f32; 3]) -> [u32; 3] {
    let longest = world[0].max(world[1]).max(world[2]);
    assert!(
        longest.is_finite() && longest > 0.0,
        "a light volume spans a positive world"
    );
    world.map(|extent| {
        ((extent / longest) * SIDE as f32)
            .round()
            .clamp(MIN_SIDE as f32, SIDE as f32) as u32
    })
}
