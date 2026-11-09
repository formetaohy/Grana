use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, ComputePassDescriptor, ComputePipeline,
    ComputePipelineDescriptor, Device, Extent3d, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    ShaderModuleDescriptor, ShaderSource, StoreOp, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::frame::FrameData;
use crate::layout::Layouts;
use crate::target::{DEPTH_FORMAT, NORMAL_FORMAT};

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

pub(crate) struct Ao {
    normal: TextureView,
    depth: TextureView,
    volume_normal: TextureView,
    read: BindGroup,
    compute: BindGroup,
    pipeline: ComputePipeline,
    uniform: Buffer,
    size: [u32; 2],
    volume_size: [u32; 2],
}

impl Ao {
    pub fn new(device: &Device, layouts: &Layouts, size: [u32; 2], volume_size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0 && volume_size[0] > 0 && volume_size[1] > 0,
            "an ambient occlusion target covers a positive surface"
        );
        assert!(
            volume_size[0] >= size[0] && volume_size[1] >= size[1],
            "the volume target holds the volume pane"
        );
        let normal = attachment(
            device,
            "grana.ao.normal",
            size,
            NORMAL_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let depth = attachment(
            device,
            "grana.ao.depth",
            size,
            DEPTH_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let volume_normal = attachment(
            device,
            "grana.ao.volume.normal",
            volume_size,
            NORMAL_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let output = attachment(
            device,
            "grana.ao.output",
            size,
            OUTPUT_FORMAT,
            TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
        );
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
                resource: BindingResource::TextureView(&output),
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
                    resource: BindingResource::TextureView(&depth),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&normal),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&output),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(&volume_normal),
                },
            ],
        });
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
            cache: None,
        });
        Self {
            normal,
            depth,
            volume_normal,
            read,
            compute,
            pipeline,
            uniform,
            size,
            volume_size,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn volume_size(&self) -> [u32; 2] {
        self.volume_size
    }

    pub fn normal(&self) -> &TextureView {
        &self.normal
    }

    pub fn depth(&self) -> &TextureView {
        &self.depth
    }

    pub fn volume_normal(&self) -> &TextureView {
        &self.volume_normal
    }

    pub fn group(&self) -> &BindGroup {
        &self.read
    }

    pub fn clear(&self, encoder: &mut CommandEncoder) {
        let _ = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ao.clear.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &self.normal,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
    }

    pub fn clear_volume(&self, encoder: &mut CommandEncoder) {
        let _ = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ao.volume.clear.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &self.volume_normal,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
    }

    pub fn evaluate(
        &self,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        frame: &FrameData,
        reach: f32,
        strength: f32,
        sample: u32,
    ) {
        assert!(
            reach.is_finite() && reach > 0.0,
            "an ambient occlusion reach is a positive screen distance"
        );
        assert!(
            strength.is_finite() && (0.0..=1.0).contains(&strength),
            "an ambient occlusion strength lies in the unit interval"
        );
        let data = AoData {
            view_projection: frame.view_projection,
            inverse_view_projection: frame.inv_view_projection,
            eye: frame.eye,
            extent: [self.size[0] as f32, self.size[1] as f32, reach, strength],
            noise: [sample as f32, frame.march[3], 0.0, 0.0],
            region: frame.viewport,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("grana.ao.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.compute, &[]);
        pass.dispatch_workgroups(
            self.size[0].div_ceil(WORKGROUP),
            self.size[1].div_ceil(WORKGROUP),
            1,
        );
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

fn attachment(
    device: &Device,
    label: &str,
    size: [u32; 2],
    format: TextureFormat,
    usage: TextureUsages,
) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default())
}
