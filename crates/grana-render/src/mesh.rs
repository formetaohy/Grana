use bytemuck::{Pod, Zeroable};
use grana_object::Mesh;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferUsages, ColorTargetState,
    ColorWrites, CompareFunction, DepthBiasState, DepthStencilState, Device, FragmentState,
    IndexFormat, LoadOp, MultisampleState, Operations, PipelineLayoutDescriptor, PrimitiveState,
    Queue, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages,
    StencilState, StoreOp, TextureView, VertexAttribute, VertexBufferLayout, VertexFormat,
    VertexState, VertexStepMode, util::DeviceExt,
};

use crate::Camera;
use crate::params::Shading;
use crate::target::{DEPTH_FORMAT, SCENE_FORMAT};
use crate::transfer::srgb_to_linear;

pub(crate) const SURFACE_LAYER: usize = 0;
pub(crate) const SPOT_TRACK_LAYER: usize = 1;
pub(crate) const SURFACE_TRACK_LAYER: usize = 2;
const LAYER_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshStyle {
    color: [f32; 3],
    visible: bool,
    selected: bool,
}

impl MeshStyle {
    pub fn new(color: [f32; 3], visible: bool, selected: bool) -> Self {
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a mesh color is an srgb triple"
        );
        Self {
            color,
            visible,
            selected,
        }
    }

    pub fn color(self) -> [f32; 3] {
        self.color
    }

    pub fn visible(self) -> bool {
        self.visible
    }

    pub fn selected(self) -> bool {
        self.selected
    }
}

#[derive(Clone, Copy)]
pub struct MeshFrame<'a> {
    pub styles: &'a [MeshStyle],
}

pub(crate) struct VolumeFrame<'a> {
    pub target: &'a TextureView,
    pub depth: &'a TextureView,
    pub region: [u32; 4],
    pub camera: &'a Camera,
    pub shading: Shading,
    pub scene_radius: f32,
}

pub(crate) struct SectionFrame<'a> {
    pub target: &'a TextureView,
    pub depth: &'a TextureView,
    pub region: [u32; 4],
    pub uniforms: &'a BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct StyleUniform {
    color: [f32; 4],
    flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MeshUniforms {
    view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    shading: [f32; 4],
}

const _: () = {
    assert!(size_of::<StyleUniform>() == 32);
    assert!(size_of::<MeshUniforms>() == 112);
};

const VOLUME_SHADER: &str = include_str!("mesh.wgsl");
const SECTION_SHADER: &str = include_str!("mesh_section.wgsl");

struct MeshLayer {
    vertices: Option<Buffer>,
    indices: Option<Buffer>,
    count: u32,
    objects: usize,
    style_buffer: Option<Buffer>,
    style_group: Option<BindGroup>,
}

impl MeshLayer {
    fn empty() -> Self {
        Self {
            vertices: None,
            indices: None,
            count: 0,
            objects: 0,
            style_buffer: None,
            style_group: None,
        }
    }

    fn set(&mut self, device: &Device, layout: &BindGroupLayout, mesh: Option<&Mesh>) {
        let Some(mesh) = mesh.filter(|mesh| !mesh.is_empty()) else {
            *self = Self::empty();
            return;
        };
        let objects = mesh
            .vertices()
            .iter()
            .map(|vertex| vertex.object())
            .max()
            .map_or(0, |object| object as usize + 1);
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.mesh.vertices"),
            contents: bytemuck::cast_slice(mesh.vertices()),
            usage: BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.mesh.indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: BufferUsages::INDEX,
        });
        let style_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.mesh.styles"),
            size: (objects.max(1) * size_of::<StyleUniform>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let style_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.mesh.style.group"),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: style_buffer.as_entire_binding(),
            }],
        });
        self.vertices = Some(vertices);
        self.indices = Some(indices);
        self.count = mesh.indices().len() as u32;
        self.objects = objects;
        self.style_buffer = Some(style_buffer);
        self.style_group = Some(style_group);
    }

    fn write_styles(&self, queue: &Queue, styles: &[MeshStyle]) {
        assert_eq!(
            styles.len(),
            self.objects,
            "every mesh object needs a style"
        );
        let Some(buffer) = self.style_buffer.as_ref() else {
            return;
        };
        let uniforms = styles
            .iter()
            .map(|style| {
                let [red, green, blue] = srgb_to_linear(style.color);
                StyleUniform {
                    color: [red, green, blue, 1.0],
                    flags: [
                        if style.visible { 1.0 } else { 0.0 },
                        if style.selected { 1.0 } else { 0.0 },
                        0.0,
                        0.0,
                    ],
                }
            })
            .collect::<Vec<_>>();
        if !uniforms.is_empty() {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&uniforms));
        }
    }

    fn draw_volume(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &RenderPipeline,
        group: &BindGroup,
        frame: &VolumeFrame<'_>,
    ) {
        let (Some(vertices), Some(indices), Some(styles)) =
            (&self.vertices, &self.indices, &self.style_group)
        else {
            return;
        };
        if self.count == 0 {
            return;
        }
        let width = frame.region[2].max(1);
        let height = frame.region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: frame.depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(
            frame.region[0] as f32,
            frame.region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(frame.region[0], frame.region[1], width, height);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(1, styles, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }

    fn draw_section(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth_pipeline: &RenderPipeline,
        color_pipeline: &RenderPipeline,
        frame: &SectionFrame<'_>,
    ) {
        let (Some(vertices), Some(indices), Some(styles)) =
            (&self.vertices, &self.indices, &self.style_group)
        else {
            return;
        };
        if self.count == 0 || frame.region[2] <= 1 || frame.region[3] <= 1 {
            return;
        }
        let width = frame.region[2];
        let height = frame.region[3];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.section.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: frame.depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(
            frame.region[0] as f32,
            frame.region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(frame.region[0], frame.region[1], width, height);
        pass.set_bind_group(0, frame.uniforms, &[]);
        pass.set_bind_group(1, styles, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.set_pipeline(depth_pipeline);
        pass.draw_indexed(0..self.count, 0, 0..1);
        pass.set_pipeline(color_pipeline);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }
}

pub(crate) struct Meshes {
    layers: [MeshLayer; LAYER_COUNT],
    uniforms: Buffer,
    uniform_group: BindGroup,
    style_layout: BindGroupLayout,
    volume_pipeline: RenderPipeline,
    depth_pipeline: RenderPipeline,
    color_pipeline: RenderPipeline,
}

impl Meshes {
    pub fn new(device: &Device, frame_layout: &BindGroupLayout) -> Self {
        let style_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.mesh.style.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.mesh.uniform.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let volume_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&style_layout)],
            immediate_size: 0,
        });
        let section_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.section.pipeline_layout"),
            bind_group_layouts: &[Some(frame_layout), Some(&style_layout)],
            immediate_size: 0,
        });
        let volume_shader = shader(device, "grana.mesh", VOLUME_SHADER);
        let section_shader = shader(device, "grana.mesh.section", SECTION_SHADER);
        let volume_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh"),
            layout: Some(&volume_layout),
            vertex: VertexState {
                module: &volume_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let depth_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.depth"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_shader,
                entry_point: Some("fs_depth"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::empty(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let color_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.color"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::Equal)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.mesh.uniforms"),
            size: size_of::<MeshUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.mesh.uniform.group"),
            layout: &uniform_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Self {
            layers: [MeshLayer::empty(), MeshLayer::empty(), MeshLayer::empty()],
            uniforms,
            uniform_group,
            style_layout,
            volume_pipeline,
            depth_pipeline,
            color_pipeline,
        }
    }

    pub fn set(&mut self, layer: usize, device: &Device, mesh: Option<&Mesh>) {
        self.layers[layer].set(device, &self.style_layout, mesh);
    }

    pub fn write_styles(&self, queue: &Queue, styles: [Option<&[MeshStyle]>; LAYER_COUNT]) {
        for (layer, styles) in self.layers.iter().zip(styles) {
            if let Some(styles) = styles {
                layer.write_styles(queue, styles);
            }
        }
    }

    pub fn draw_volume(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &Queue,
        frame: VolumeFrame<'_>,
        styles: [Option<&[MeshStyle]>; LAYER_COUNT],
    ) {
        if styles.iter().all(Option::is_none) {
            return;
        }
        let width = frame.region[2].max(1);
        let height = frame.region[3].max(1);
        let aspect = width as f32 / height as f32;
        let light = frame.camera.light_direction();
        let eye = frame.camera.eye();
        let uniforms = MeshUniforms {
            view_projection: frame
                .camera
                .view_projection(aspect, frame.scene_radius)
                .to_cols_array_2d(),
            eye: eye.extend(0.0).to_array(),
            light: light.extend(0.0).to_array(),
            shading: [
                frame.shading.ambient,
                frame.shading.diffuse,
                frame.shading.specular,
                frame.shading.shininess,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        for (layer, styles) in self.layers.iter().zip(styles) {
            if styles.is_some() {
                layer.draw_volume(encoder, &self.volume_pipeline, &self.uniform_group, &frame);
            }
        }
    }

    pub fn draw_section(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: SectionFrame<'_>,
        styles: [Option<&[MeshStyle]>; LAYER_COUNT],
    ) {
        for (layer, styles) in self.layers.iter().zip(styles) {
            if styles.is_some() {
                layer.draw_section(encoder, &self.depth_pipeline, &self.color_pipeline, &frame);
            }
        }
    }
}

fn shader(device: &Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    })
}

fn depth_state(write: bool, compare: CompareFunction) -> DepthStencilState {
    DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: StencilState::default(),
        bias: DepthBiasState::default(),
    }
}

fn vertex_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: size_of::<grana_object::MeshVertex>() as u64,
        step_mode: VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                format: VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            VertexAttribute {
                format: VertexFormat::Uint32,
                offset: 24,
                shader_location: 2,
            },
        ],
    }
}
