use bytemuck::{Pod, Zeroable};
use grana_gpu::{IndexedCommands, Table};
use grana_object::{Mesh, MeshVertex};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, ComputePipeline,
    ComputePipelineDescriptor, Device, FragmentState, IndexFormat, LoadOp, MultisampleState,
    Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp, TextureView,
    VertexAttribute, VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
    util::DeviceExt,
};

use crate::layout::Layouts;
use crate::schema::{Layer, LayerKey, SceneFrame, Shading, Style};
use crate::target::SCENE_FORMAT;
use crate::transfer::srgb_to_linear;
use crate::volume::depth_state;

const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_cull.wgsl")
);
const VOLUME_SHADER: &str = include_str!("shader/mesh.wgsl");
const SECTION_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_section.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ObjectData {
    center: [f32; 4],
    radius: f32,
    first_index: u32,
    index_count: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct StyleData {
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
    assert!(size_of::<ObjectData>() == 32);
    assert!(size_of::<StyleData>() == 32);
    assert!(size_of::<MeshUniforms>() == 112);
};

pub(crate) struct MeshPasses {
    layers: Vec<MeshLayer>,
    cull: ComputePipeline,
    section_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    section_depth: RenderPipeline,
    section_color: RenderPipeline,
    uniforms: Buffer,
    uniform_group: BindGroup,
}

impl MeshPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grana.mesh.uniform.layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let volume_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&layouts.styles)],
            immediate_size: 0,
        });
        let section_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.section.pipeline_layout"),
            bind_group_layouts: &[Some(&layouts.scene), Some(&layouts.styles)],
            immediate_size: 0,
        });
        let cull_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.cull"),
            source: ShaderSource::Wgsl(CULL_SHADER.into()),
        });
        let cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.cull.volume"),
            layout: Some(&layouts.mesh_cull),
            module: &cull_module,
            entry_point: Some("mesh_cull_volume"),
            compilation_options: Default::default(),
            cache: None,
        });
        let section_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.cull.section"),
            layout: Some(&layouts.mesh_cull),
            module: &cull_module,
            entry_point: Some("mesh_cull_section"),
            compilation_options: Default::default(),
            cache: None,
        });
        let volume_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh"),
            source: ShaderSource::Wgsl(VOLUME_SHADER.into()),
        });
        let section_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.section"),
            source: ShaderSource::Wgsl(SECTION_SHADER.into()),
        });
        let volume_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh"),
            layout: Some(&volume_layout),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_module,
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
        let section_depth = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.depth"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_module,
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
        let section_color = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.color"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::Equal)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_module,
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
            layers: Vec::new(),
            cull,
            section_cull,
            volume_pipeline,
            section_depth,
            section_color,
            uniforms,
            uniform_group,
        }
    }

    pub fn set(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
    ) {
        self.layers
            .resize_with(scene.layers.len(), || MeshLayer::new(device, layouts));
        for (layer, source) in self.layers.iter_mut().zip(scene.layers) {
            layer.set(device, queue, layouts, *source, scene.styles);
        }
    }

    pub fn write_uniforms(&self, queue: &Queue, frame: &crate::frame::FrameData, shading: Shading) {
        let uniforms = MeshUniforms {
            view_projection: frame.view_projection,
            eye: frame.eye,
            light: frame.light,
            shading: [
                shading.ambient,
                shading.diffuse,
                shading.specular,
                shading.shininess,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.mesh.cull.volume.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.cull);
        pass.set_bind_group(0, scene, &[]);
        for layer in &self.layers {
            if layer.volume_commands.is_none() {
                continue;
            }
            let Some(group) = layer.volume_command_group.as_ref() else {
                continue;
            };
            pass.set_bind_group(1, &layer.object_group, &[]);
            pass.set_bind_group(2, &layer.style_group, &[]);
            pass.set_bind_group(3, group, &[]);
            pass.dispatch_workgroups(layer.object_count.div_ceil(64), 1, 1);
        }
    }

    pub fn cull_section(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.mesh.cull.section.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.section_cull);
        pass.set_bind_group(0, scene, &[]);
        for layer in &self.layers {
            if layer.section_commands.is_none() {
                continue;
            }
            let Some(group) = layer.section_command_group.as_ref() else {
                continue;
            };
            pass.set_bind_group(1, &layer.object_group, &[]);
            pass.set_bind_group(2, &layer.style_group, &[]);
            pass.set_bind_group(3, group, &[]);
            pass.dispatch_workgroups(layer.object_count.div_ceil(64), 1, 1);
        }
    }

    pub fn draw_volume(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        depth: &TextureView,
        region: [u32; 4],
    ) {
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_pipeline(&self.volume_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for layer in &self.layers {
            layer.draw(&mut pass);
        }
    }

    pub fn draw_section(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        depth: &TextureView,
        region: [u32; 4],
        scene: &BindGroup,
    ) {
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.section.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_bind_group(0, scene, &[]);
        for layer in &self.layers {
            layer.draw_section(&mut pass, &self.section_depth, &self.section_color);
        }
    }
}

struct MeshLayer {
    key: Option<LayerKey>,
    vertices: Option<Buffer>,
    indices: Option<Buffer>,
    objects: Table<ObjectData>,
    styles: Table<StyleData>,
    object_group: BindGroup,
    style_group: BindGroup,
    volume_commands: Option<IndexedCommands>,
    section_commands: Option<IndexedCommands>,
    volume_command_group: Option<BindGroup>,
    section_command_group: Option<BindGroup>,
    object_count: u32,
    cached_styles: Vec<StyleData>,
}

impl MeshLayer {
    fn new(device: &Device, layouts: &Layouts) -> Self {
        let objects = Table::new(device, "grana.mesh.objects");
        let styles = Table::new(device, "grana.mesh.styles");
        Self {
            key: None,
            vertices: None,
            indices: None,
            object_group: bind_group(device, &layouts.objects, objects.buffer()),
            style_group: bind_group(device, &layouts.styles, styles.buffer()),
            objects,
            styles,
            volume_commands: None,
            section_commands: None,
            volume_command_group: None,
            section_command_group: None,
            object_count: 0,
            cached_styles: Vec::new(),
        }
    }

    fn set(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        layer: Layer<'_>,
        styles: &[Style],
    ) {
        let (start, count) = (layer.style_start, layer.style_count);
        assert!(
            start + count <= styles.len(),
            "a mesh layer names its style rows"
        );
        if self.key != Some(layer.key) {
            self.key = Some(layer.key);
            self.upload(device, queue, layouts, layer.mesh);
        }
        assert_eq!(
            count, self.object_count as usize,
            "every mesh object carries one style"
        );
        let data = styles[start..start + count]
            .iter()
            .map(|style| style_data(*style))
            .collect::<Vec<_>>();
        if data != self.cached_styles {
            let grown = self.styles.write(device, queue, &data);
            self.cached_styles = data;
            if grown {
                self.style_group = bind_group(device, &layouts.styles, self.styles.buffer());
            }
        }
    }

    fn upload(&mut self, device: &Device, queue: &Queue, layouts: &Layouts, mesh: Option<&Mesh>) {
        self.reset();
        let Some(mesh) = mesh.filter(|mesh| !mesh.is_empty()) else {
            return;
        };
        let vertices = mesh.vertices();
        let objects = object_count(vertices);
        assert!(objects > 0, "a non empty mesh names at least one object");
        let (indices, ranges) = group_indices(vertices, mesh.indices(), objects);
        self.vertices = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.mesh.vertices"),
                contents: bytemuck::cast_slice(vertices),
                usage: BufferUsages::VERTEX,
            }),
        );
        self.indices = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.mesh.indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: BufferUsages::INDEX,
            }),
        );
        self.object_count = objects as u32;
        let volume_commands = IndexedCommands::new(device, "grana.mesh.volume.commands", objects);
        let section_commands = IndexedCommands::new(device, "grana.mesh.section.commands", objects);
        self.volume_command_group = Some(bind_group(
            device,
            &layouts.commands,
            volume_commands.buffer(),
        ));
        self.section_command_group = Some(bind_group(
            device,
            &layouts.commands,
            section_commands.buffer(),
        ));
        self.volume_commands = Some(volume_commands);
        self.section_commands = Some(section_commands);
        let bounds = bounds(vertices, objects);
        let data = ranges
            .iter()
            .zip(bounds)
            .map(|(range, bound)| ObjectData {
                center: [bound.center.x, bound.center.y, bound.center.z, 0.0],
                radius: bound.radius,
                first_index: range.0,
                index_count: range.1,
                reserved: 0,
            })
            .collect::<Vec<_>>();
        let grown = self.objects.write(device, queue, &data);
        if grown {
            self.object_group = bind_group(device, &layouts.objects, self.objects.buffer());
        }
    }

    fn reset(&mut self) {
        self.vertices = None;
        self.indices = None;
        self.volume_commands = None;
        self.section_commands = None;
        self.volume_command_group = None;
        self.section_command_group = None;
        self.object_count = 0;
        self.cached_styles.clear();
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let (Some(vertices), Some(indices), Some(commands)) = (
            self.vertices.as_ref(),
            self.indices.as_ref(),
            self.volume_commands.as_ref(),
        ) else {
            return;
        };
        pass.set_bind_group(1, &self.style_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(commands.buffer(), 0, self.object_count);
    }

    fn draw_section<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        depth_pipeline: &RenderPipeline,
        color_pipeline: &RenderPipeline,
    ) {
        let (Some(vertices), Some(indices), Some(commands)) = (
            self.vertices.as_ref(),
            self.indices.as_ref(),
            self.section_commands.as_ref(),
        ) else {
            return;
        };
        pass.set_bind_group(1, &self.style_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.set_pipeline(depth_pipeline);
        pass.multi_draw_indexed_indirect(commands.buffer(), 0, self.object_count);
        pass.set_pipeline(color_pipeline);
        pass.multi_draw_indexed_indirect(commands.buffer(), 0, self.object_count);
    }
}

fn bind_group(device: &Device, layout: &BindGroupLayout, buffer: &Buffer) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.mesh.group"),
        layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn object_count(vertices: &[MeshVertex]) -> usize {
    vertices
        .iter()
        .map(|vertex| vertex.object())
        .max()
        .map_or(0, |object| object as usize + 1)
}

fn group_indices(
    vertices: &[MeshVertex],
    indices: &[u32],
    objects: usize,
) -> (Vec<u32>, Vec<(u32, u32)>) {
    let mut counts = vec![0u32; objects];
    for triangle in indices.as_chunks::<3>().0 {
        let object = vertices[triangle[0] as usize].object() as usize;
        counts[object] += 1;
    }
    let mut starts = vec![0u32; objects];
    let mut next = 0u32;
    for (start, count) in starts.iter_mut().zip(&counts) {
        *start = next;
        next += count * 3;
    }
    let mut grouped = vec![0u32; indices.len()];
    let mut cursor = starts.clone();
    for triangle in indices.as_chunks::<3>().0 {
        let object = vertices[triangle[0] as usize].object() as usize;
        let at = cursor[object] as usize;
        grouped[at..at + 3].copy_from_slice(triangle);
        cursor[object] += 3;
    }
    let ranges = starts
        .iter()
        .zip(&counts)
        .map(|(start, count)| (*start, *count * 3))
        .collect();
    (grouped, ranges)
}

#[derive(Clone, Copy)]
struct Bounds {
    center: glam::Vec3,
    radius: f32,
}

fn bounds(vertices: &[MeshVertex], objects: usize) -> Vec<Bounds> {
    let mut low = vec![glam::Vec3::splat(f32::INFINITY); objects];
    let mut high = vec![glam::Vec3::splat(f32::NEG_INFINITY); objects];
    for vertex in vertices {
        let object = vertex.object() as usize;
        let position = glam::Vec3::from(vertex.position());
        low[object] = low[object].min(position);
        high[object] = high[object].max(position);
    }
    low.iter()
        .zip(&high)
        .map(|(low, high)| {
            if low.is_finite() && high.is_finite() {
                Bounds {
                    center: 0.5 * (*low + *high),
                    radius: (0.5 * (*high - *low)).length(),
                }
            } else {
                Bounds {
                    center: glam::Vec3::ZERO,
                    radius: 0.0,
                }
            }
        })
        .collect()
}

fn style_data(style: Style) -> StyleData {
    let [red, green, blue] = srgb_to_linear(style.color());
    StyleData {
        color: [red, green, blue, 1.0],
        flags: [
            if style.visible() { 1.0 } else { 0.0 },
            if style.selected() { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ],
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
