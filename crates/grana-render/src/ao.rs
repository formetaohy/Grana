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

const GTAO: &str = concat!(
    include_str!("shader/ao_common.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/geometry.wgsl"),
    "\n",
    include_str!("shader/surface.wgsl"),
    "\n",
    include_str!("shader/gtao.wgsl")
);

const FILTER: &str = concat!(
    include_str!("shader/ao_common.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/geometry.wgsl"),
    "\n",
    include_str!("shader/surface.wgsl"),
    "\n",
    include_str!("shader/ao_filter.wgsl")
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
    _half: Texture,
    _full: Texture,
    half: TextureView,
    view: TextureView,
    read: BindGroup,
    data: BindGroup,
    filter: BindGroup,
    pipeline: RenderPipeline,
    filter_pipeline: RenderPipeline,
    uniform: Buffer,
    bytes: u64,
}

pub(crate) struct AoPipeline {
    gtao: RenderPipeline,
    filter: RenderPipeline,
}

impl AoPipeline {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let gtao = pipeline(
            device,
            &layouts.ao_pipeline,
            "grana.ao",
            "gtao_main",
            GTAO,
            pipelines,
        );
        let filter = pipeline(
            device,
            &layouts.ao_filter_pipeline,
            "grana.ao.filter",
            "fs_filter",
            FILTER,
            pipelines,
        );
        Self { gtao, filter }
    }
}

impl Ao {
    pub fn new(device: &Device, layouts: &Layouts, pipeline: &AoPipeline, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "an ambient occlusion target covers a positive surface"
        );
        let half = [size[0].div_ceil(2), size[1].div_ceil(2)];
        let half_texture = output(device, "grana.ao.half", half);
        let full_texture = output(device, "grana.ao.full", size);
        let half_view = half_texture.create_view(&TextureViewDescriptor::default());
        let view = full_texture.create_view(&TextureViewDescriptor::default());
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
                resource: BindingResource::TextureView(&view),
            }],
        });
        let data = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ao.data.group"),
            layout: &layouts.ao_data,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let filter = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ao.filter.group"),
            layout: &layouts.ao_filter,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&half_view),
                },
            ],
        });
        Self {
            _half: half_texture,
            _full: full_texture,
            half: half_view,
            view,
            read,
            data,
            filter,
            pipeline: pipeline.gtao.clone(),
            filter_pipeline: pipeline.filter.clone(),
            uniform,
            bytes: (crate::target::bytes(OUTPUT_FORMAT, half)
                + crate::target::bytes(OUTPUT_FORMAT, size)),
        }
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
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
        surface: &BindGroup,
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
                frame.sampling[0],
                1.0 - frame.projection[0],
                frame.sampling[1],
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let region = frame.viewport.map(|value| value as u32);
        let first = [region[0] / 2, region[1] / 2];
        let last = [
            ((region[0] + region[2] - 1) / 2).min(resolution[0].div_ceil(2) - 1),
            ((region[1] + region[3] - 1) / 2).min(resolution[1].div_ceil(2) - 1),
        ];
        self.pass(
            encoder,
            Stage {
                label: "grana.ao.pass",
                target: &self.half,
                pipeline: &self.pipeline,
                group: &self.data,
            },
            surface,
            [
                first[0],
                first[1],
                last[0] - first[0] + 1,
                last[1] - first[1] + 1,
            ],
        );
        self.pass(
            encoder,
            Stage {
                label: "grana.ao.filter.pass",
                target: &self.view,
                pipeline: &self.filter_pipeline,
                group: &self.filter,
            },
            surface,
            region,
        );
    }

    fn pass(
        &self,
        encoder: &mut CommandEncoder,
        stage: Stage<'_>,
        surface: &BindGroup,
        region: [u32; 4],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(stage.label),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: stage.target,
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
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(stage.pipeline);
        pass.set_bind_group(0, stage.group, &[]);
        pass.set_bind_group(1, surface, &[]);
        pass.draw(0..SIDE, 0..1);
    }
}

struct Stage<'a> {
    label: &'static str,
    target: &'a TextureView,
    pipeline: &'a RenderPipeline,
    group: &'a BindGroup,
}

fn output(device: &Device, label: &'static str, size: [u32; 2]) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
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
    })
}

fn pipeline(
    device: &Device,
    layout: &wgpu::PipelineLayout,
    label: &str,
    fragment: &str,
    source: &str,
    pipelines: &Pipelines,
) -> RenderPipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
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
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format: OUTPUT_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: pipelines.cache(),
    })
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
