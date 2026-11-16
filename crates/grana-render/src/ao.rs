use bytemuck::{Pod, Zeroable};
use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FragmentState,
    LoadOp, MultisampleState, Operations, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor, VertexState,
};

use crate::frame::FrameData;
use crate::layout::Layouts;

const SIDE: u32 = 3;
const OUTPUT_FORMAT: TextureFormat = TextureFormat::R8Unorm;
const SHADER: &str = concat!(
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/gtao.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct AoData {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    extent: [f32; 4],
    region: [f32; 4],
    noise: [f32; 4],
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
    view: TextureView,
    read: BindGroup,
    write: BindGroup,
    pipeline: RenderPipeline,
    uniform: Buffer,
}

pub(crate) struct AoPipeline {
    pipeline: RenderPipeline,
}

impl AoPipeline {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ao"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.ao"),
            layout: Some(&layouts.ao_pipeline),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("gtao_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        Self { pipeline }
    }

    pub fn pipeline(&self) -> &RenderPipeline {
        &self.pipeline
    }
}

impl Ao {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        pipeline: &RenderPipeline,
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
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
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
        let write = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ao.write.group"),
            layout: &layouts.ao_write,
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
                    resource: BindingResource::TextureView(volume),
                },
            ],
        });
        let pipeline = pipeline.clone();
        Self {
            _texture: output,
            view: output_view,
            read,
            write,
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
            region: frame.viewport,
            noise: [
                ambient.sample as f32,
                frame.march[3],
                1.0 - frame.projection[0],
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ao.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::WHITE),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_viewport(
            frame.viewport[0],
            frame.viewport[1],
            frame.viewport[2],
            frame.viewport[3],
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            frame.viewport[0] as u32,
            frame.viewport[1] as u32,
            frame.viewport[2] as u32,
            frame.viewport[3] as u32,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.write, &[]);
        pass.draw(0..SIDE, 0..1);
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
        &[255u8],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(1),
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
