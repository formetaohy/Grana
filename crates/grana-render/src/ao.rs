use bytemuck::{Pod, Zeroable};
use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, ComputePassDescriptor, ComputePipeline,
    ComputePipelineDescriptor, Device, Extent3d, Queue, ShaderModuleDescriptor, ShaderSource,
    Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::frame::FrameData;
use crate::layout::Layouts;

const WORKGROUP: u32 = 8;
const OUTPUT_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
const SHADER: &str = include_str!("shader/gtao.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct AoData {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    extent: [f32; 4],
    noise: [f32; 4],
    region: [f32; 4],
}

const _: () = assert!(size_of::<AoData>() == 192);

#[derive(Clone, Copy, Debug)]
pub(crate) struct Ambient {
    pub(crate) reach: f32,
    pub(crate) strength: f32,
    pub(crate) sample: u32,
}

impl Ambient {
    pub(crate) fn new(reach: f32, strength: f32, sample: u32) -> Self {
        assert!(
            reach.is_finite() && reach > 0.0,
            "an ambient occlusion reach is a positive screen distance"
        );
        assert!(
            strength.is_finite() && (0.0..=1.0).contains(&strength),
            "an ambient occlusion strength lies in the unit interval"
        );
        Self {
            reach,
            strength,
            sample,
        }
    }
}

pub(crate) struct Ao {
    _texture: Texture,
    read: BindGroup,
    compute: BindGroup,
    pipeline: ComputePipeline,
    uniform: Buffer,
}

pub(crate) struct AoPipeline {
    pipeline: ComputePipeline,
}

impl AoPipeline {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ao"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.ao"),
            layout: Some(&layouts.ao_pipeline),
            module: &module,
            entry_point: Some("gtao_main"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        Self { pipeline }
    }

    pub fn pipeline(&self) -> &ComputePipeline {
        &self.pipeline
    }
}

impl Ao {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        pipeline: &ComputePipeline,
        normal: &TextureView,
        depth: &TextureView,
        volume: &TextureView,
        size: [u32; 2],
    ) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "an ambient occlusion target covers a positive surface"
        );
        let output = device.create_texture(&TextureDescriptor {
            label: Some("grana.ao.output"),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output_view = output.create_view(&TextureViewDescriptor::default());
        let uniform = device.create_buffer(&BufferDescriptor {
            label: Some("grana.ao.uniforms"),
            size: size_of::<AoData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let read = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ao.read.group"),
            layout: &layouts.ao_read,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&output_view),
            }],
        });
        let compute = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ao.compute.group"),
            layout: &layouts.ao_compute,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(depth),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(normal),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&output_view),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(volume),
                },
            ],
        });
        let pipeline = pipeline.clone();
        Self {
            _texture: output,
            read,
            compute,
            pipeline,
            uniform,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.read
    }

    pub fn evaluate(
        &self,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        frame: &FrameData,
        resolution: [u32; 2],
        ambient: Ambient,
    ) {
        assert!(
            frame.viewport[0] + frame.viewport[2] <= resolution[0] as f32
                && frame.viewport[1] + frame.viewport[3] <= resolution[1] as f32,
            "the ambient occlusion view lies on the target"
        );
        let data = AoData {
            view_projection: frame.view_projection,
            inverse_view_projection: frame.inv_view_projection,
            eye: frame.eye,
            extent: [
                resolution[0] as f32,
                resolution[1] as f32,
                ambient.reach,
                ambient.strength,
            ],
            noise: [
                ambient.sample as f32,
                frame.march[3],
                1.0 - frame.projection[0],
                0.0,
            ],
            region: frame.viewport,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("grana.ao.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.compute, &[]);
        let width = frame.viewport[2] as u32;
        let height = frame.viewport[3] as u32;
        pass.dispatch_workgroups(width.div_ceil(WORKGROUP), height.div_ceil(WORKGROUP), 1);
    }
}

pub(crate) fn dummy(device: &Device, queue: &Queue, layouts: &Layouts) -> BindGroup {
    let texture: Texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.ao.dummy"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255u8; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.ao.dummy.group"),
        layout: &layouts.ao_read,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(
                &texture.create_view(&TextureViewDescriptor::default()),
            ),
        }],
    })
}
