use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use grana_gpu::layout::{sampler, texture};
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FilterMode,
    FragmentState, LoadOp, MultisampleState, Operations, Origin3d, PipelineLayoutDescriptor,
    PrimitiveState, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
    ShaderStages, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
    VertexBufferLayout, VertexState, VertexStepMode, util::DeviceExt,
};

use crate::schema::{Digest, Overlay};
use crate::target::SCENE_FORMAT;

const SHADER: &str = include_str!("shader/overlay.wgsl");

const SHEET_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

const ROW_ALIGNMENT: u32 = 256;

const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

const CORNERS: [usize; 6] = [0, 1, 2, 0, 2, 3];

const ADDITIVE: BlendState = BlendState {
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
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 4],
    uv: [f32; 2],
    tint: [f32; 4],
}

const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Float32x4];

struct Sheet {
    group: BindGroup,
}

struct Draw {
    sheet: u64,
    first: u32,
}

pub(crate) struct Overlays {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    sampler: Sampler,
    sheets: HashMap<u64, Sheet>,
    draws: Vec<Draw>,
    vertices: Vec<Vertex>,
    buffer: Option<Buffer>,
    stamp: u64,
}

impl Overlays {
    pub fn new(device: &Device) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.overlay.layout"),
            entries: &[
                texture(0, ShaderStages::FRAGMENT),
                sampler(1, ShaderStages::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.overlay.pipeline.layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.overlay"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.overlay"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(ADDITIVE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.overlay.sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            sheets: HashMap::new(),
            draws: Vec::new(),
            vertices: Vec::new(),
            buffer: None,
            stamp: 0,
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        overlays: &[Overlay<'_>],
        view_projection: Mat4,
    ) {
        let stamp = stamp(overlays, view_projection);
        if self.stamp == stamp {
            return;
        }
        self.stamp = stamp;
        self.vertices.clear();
        self.draws.clear();
        self.buffer = None;
        if overlays.is_empty() {
            self.sheets.clear();
            return;
        }
        let layout = &self.layout;
        let sampler = &self.sampler;
        let mut kept = Vec::with_capacity(overlays.len());
        for overlay in overlays {
            let key = overlay.key();
            self.sheets
                .entry(key)
                .or_insert_with(|| sheet(device, queue, layout, sampler, overlay));
            kept.push(key);
            let first = u32::try_from(self.vertices.len()).expect("an overlay holds u32 vertices");
            let tint = [
                overlay.tint()[0],
                overlay.tint()[1],
                overlay.tint()[2],
                overlay.opacity(),
            ];
            for corner in CORNERS {
                let world = Vec3::from(overlay.corners()[corner]);
                let clip = view_projection * Vec4::from((world, 1.0));
                self.vertices.push(Vertex {
                    position: clip.to_array(),
                    uv: UVS[corner],
                    tint,
                });
            }
            self.draws.push(Draw { sheet: key, first });
        }
        self.sheets.retain(|key, _| kept.contains(key));
        self.buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.overlay.vertices"),
                contents: bytemuck::cast_slice(&self.vertices),
                usage: BufferUsages::VERTEX,
            }),
        );
    }

    pub fn draw(&self, encoder: &mut CommandEncoder, target: &TextureView, viewport: [u32; 4]) {
        let Some(buffer) = self.buffer.as_ref().filter(|_| !self.draws.is_empty()) else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.overlay.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_viewport(
            viewport[0] as f32,
            viewport[1] as f32,
            viewport[2] as f32,
            viewport[3] as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(viewport[0], viewport[1], viewport[2], viewport[3]);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for draw in &self.draws {
            let Some(sheet) = self.sheets.get(&draw.sheet) else {
                continue;
            };
            pass.set_bind_group(0, &sheet.group, &[]);
            pass.draw(draw.first..draw.first + CORNERS.len() as u32, 0..1);
        }
    }
}

fn stamp(overlays: &[Overlay<'_>], view_projection: Mat4) -> u64 {
    let mut digest = Digest::new();
    digest.mix(overlays.len() as u64);
    for overlay in overlays {
        digest.mix(overlay.digest());
    }
    for value in view_projection.to_cols_array() {
        digest.real(value);
    }
    digest.finish()
}

fn sheet(
    device: &Device,
    queue: &Queue,
    layout: &BindGroupLayout,
    sampler: &Sampler,
    overlay: &Overlay<'_>,
) -> Sheet {
    let [width, height] = overlay.size();
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.overlay.sheet"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: SHEET_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let row = width * 4;
    let stride = row.div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT;
    let layout_of = |stride: u32| TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(stride),
        rows_per_image: Some(height),
    };
    let extent = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let write = TexelCopyTextureInfo {
        texture: &texture,
        mip_level: 0,
        origin: Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    };
    if stride == row {
        queue.write_texture(write, overlay.pixels(), layout_of(row), extent);
    } else {
        let mut padded = vec![0u8; stride as usize * height as usize];
        for line in 0..height as usize {
            let from = line * row as usize;
            let to = line * stride as usize;
            padded[to..to + row as usize]
                .copy_from_slice(&overlay.pixels()[from..from + row as usize]);
        }
        queue.write_texture(write, &padded, layout_of(stride), extent);
    }
    let view = texture.create_view(&TextureViewDescriptor::default());
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.overlay.group"),
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Sheet { group }
}
