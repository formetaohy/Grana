use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use grana_geometry::PackedMesh;
use grana_gpu::{IndexedCommands, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, ComputePipeline,
    ComputePipelineDescriptor, DepthBiasState, DepthStencilState, Device, FragmentState,
    IndexFormat, LoadOp, MultisampleState, Operations, PipelineLayoutDescriptor, PrimitiveState,
    Queue, RenderPass, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StencilFaceState, StencilOperation, StencilState, StoreOp, TextureFormat,
    TextureView, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
    util::DeviceExt,
};

use crate::cut;
use crate::frame::SectionData;
use crate::layout::Layouts;
use crate::schema::{LayerKey, SceneFrame, Style};
use crate::target::SCENE_FORMAT;
use crate::volume::depth_state;
use grana_display::Shading;
use grana_display::srgb_to_linear;

const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_cull.wgsl")
);
const VOLUME_SHADER: &str = concat!(
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/mesh.wgsl")
);
const SECTION_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_section.wgsl")
);

const VOLUME_NARROW: usize = 0;
const VOLUME_WIDE: usize = 1;
const SHADOW_NARROW: usize = 2;
const SHADOW_WIDE: usize = 3;
const REGIONS: usize = 4;
const UNIT: f32 = 65535.0;
const WORKGROUP: u32 = 64;
const STENCIL: TextureFormat = TextureFormat::Stencil8;
const SCISSOR_MARGIN: f32 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PointData {
    position: [u16; 4],
    normal: [u16; 2],
    object: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ObjectData {
    center_radius: [f32; 4],
    minimum: [f32; 4],
    extent: [f32; 4],
    narrow_first: u32,
    narrow_count: u32,
    wide_first: u32,
    wide_count: u32,
    vertex_base: u32,
    reserved: [u32; 3],
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
    region_min: [f32; 4],
    region_max: [f32; 4],
    jitter: [f32; 4],
    light_view: [[f32; 4]; 4],
    light_reach: [f32; 4],
    shadow: [f32; 4],
}

const _: () = {
    assert!(size_of::<PointData>() == 16);
    assert!(size_of::<ObjectData>() == 80);
    assert!(size_of::<StyleData>() == 32);
    assert!(size_of::<MeshUniforms>() == 256);
};

#[derive(Clone, Copy)]
struct SectionObject {
    first: u32,
    count: u32,
    base: i32,
    wide: bool,
    low: [f32; 3],
    high: [f32; 3],
}

pub(crate) struct MeshPasses {
    slots: HashMap<LayerKey, MeshSlot>,
    active: Vec<LayerKey>,
    order: u64,
    budget: u64,
    resident: u64,
    cull: ComputePipeline,
    shadow_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    shadow_pipeline: RenderPipeline,
    section_clear: RenderPipeline,
    section_parity: RenderPipeline,
    section_fill: RenderPipeline,
    uniforms: Buffer,
    uniform_group: BindGroup,
}

impl MeshPasses {
    pub fn new(device: &Device, layouts: &Layouts, budget: u64) -> Self {
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
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
                Some(&layouts.shadow),
            ],
            immediate_size: 0,
        });
        let shadow_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.shadow.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
            ],
            immediate_size: 0,
        });
        let section_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.section.pipeline_layout"),
            bind_group_layouts: &[
                Some(&layouts.scene),
                Some(&layouts.objects),
                Some(&layouts.styles),
            ],
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
        let shadow_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.cull.shadow"),
            layout: Some(&layouts.mesh_cull),
            module: &cull_module,
            entry_point: Some("mesh_cull_shadow"),
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
        let shadow_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.shadow"),
            layout: Some(&shadow_layout),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
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
        let section_clear = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.clear"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(cut_stencil(
                CompareFunction::Always,
                StencilOperation::Replace,
                0xFF,
            )),
            multisample: cut::multisample(),
            fragment: Some(FragmentState {
                module: &section_module,
                entry_point: Some("fs_quad"),
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
        let section_parity = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.parity"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_cut"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(cut_stencil(
                CompareFunction::Always,
                StencilOperation::Invert,
                0xFF,
            )),
            multisample: cut::multisample(),
            fragment: Some(FragmentState {
                module: &section_module,
                entry_point: Some("fs_parity"),
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
        let section_fill = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.section.fill"),
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_fill"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(cut_stencil(
                CompareFunction::NotEqual,
                StencilOperation::Keep,
                0x00,
            )),
            multisample: cut::multisample(),
            fragment: Some(FragmentState {
                module: &section_module,
                entry_point: Some("fs_fill"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            slots: HashMap::new(),
            active: Vec::new(),
            order: 0,
            budget,
            resident: 0,
            cull,
            shadow_cull,
            volume_pipeline,
            shadow_pipeline,
            section_clear,
            section_parity,
            section_fill,
            uniforms,
            uniform_group,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.resident
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.active.clear();
        self.resident = 0;
    }

    pub fn set(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
    ) {
        self.active.clear();
        self.order += 1;
        let order = self.order;
        let mut keys = HashSet::new();
        for layer in scene.layers {
            assert!(
                !layer.mesh.is_empty(),
                "a mesh layer carries packed geometry"
            );
            assert_eq!(
                layer.style_count,
                layer.mesh.object_count(),
                "every mesh object carries one style"
            );
            assert!(keys.insert(layer.key), "a scene names each mesh layer once");
            assert!(
                layer.style_start + layer.style_count <= scene.styles.len(),
                "a mesh layer names its style rows"
            );
            if !self.slots.contains_key(&layer.key) {
                let slot = MeshSlot::new(device, queue, layouts, layer.mesh);
                self.resident += slot.bytes;
                self.slots.insert(layer.key, slot);
            }
            let slot = self
                .slots
                .get_mut(&layer.key)
                .expect("the mesh slot exists");
            slot.touch(order);
            slot.write_styles(
                queue,
                &scene.styles[layer.style_start..layer.style_start + layer.style_count],
            );
            self.active.push(layer.key);
        }
        self.evict();
    }

    fn evict(&mut self) {
        while self.resident > self.budget {
            let evicted = self
                .slots
                .iter()
                .filter(|(key, _)| !self.active.contains(key))
                .min_by_key(|(_, slot)| slot.used)
                .map(|(key, _)| *key);
            let Some(key) = evicted else {
                break;
            };
            if let Some(slot) = self.slots.remove(&key) {
                self.resident = self.resident.saturating_sub(slot.bytes);
            }
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
            region_min: frame.region_min,
            region_max: frame.region_max,
            jitter: frame.jitter,
            light_view: frame.light_view,
            light_reach: frame.light_reach,
            shadow: [shading.shadows, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull_pass(encoder, scene, &self.cull, "grana.mesh.cull.volume.pass");
    }

    pub fn cull_shadow(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull_pass(
            encoder,
            scene,
            &self.shadow_cull,
            "grana.mesh.cull.shadow.pass",
        );
    }

    fn cull_pass(
        &self,
        encoder: &mut CommandEncoder,
        scene: &BindGroup,
        pipeline: &ComputePipeline,
        label: &str,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, scene, &[]);
        for key in &self.active {
            let slot = &self.slots[key];
            pass.set_bind_group(1, &slot.object_group, &[]);
            pass.set_bind_group(2, &slot.style_group, &[]);
            pass.set_bind_group(3, &slot.command_group, &[]);
            pass.dispatch_workgroups(slot.object_count.div_ceil(WORKGROUP), 1, 1);
        }
    }

    pub fn draw_shadow(&self, encoder: &mut CommandEncoder, target: &TextureView) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.shadow.pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for key in &self.active {
            self.slots[key].draw(&mut pass, SHADOW_NARROW, SHADOW_WIDE);
        }
    }

    pub fn draw_volume(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        depth: &TextureView,
        region: [u32; 4],
        shadow: &BindGroup,
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
        pass.set_bind_group(3, shadow, &[]);
        for key in &self.active {
            self.slots[key].draw(&mut pass, VOLUME_NARROW, VOLUME_WIDE);
        }
    }

    pub fn draw_section<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        section: &SectionData,
        size: [u32; 2],
    ) {
        pass.set_bind_group(0, scene, &[]);
        for key in &self.active {
            let slot = &self.slots[key];
            pass.set_bind_group(1, &slot.object_group, &[]);
            pass.set_bind_group(2, &slot.style_group, &[]);
            for (object, object_data) in slot.section.iter().enumerate() {
                if !slot.visible[object] {
                    continue;
                }
                let Some(rect) = cut_rect(object_data, section, size) else {
                    continue;
                };
                pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
                pass.set_pipeline(&self.section_clear);
                pass.draw(0..3, 0..1);
                pass.set_pipeline(&self.section_parity);
                slot.draw_object(pass, object);
                pass.set_pipeline(&self.section_fill);
                pass.draw(0..3, object as u32..object as u32 + 1);
            }
        }
    }
}

struct MeshSlot {
    vertices: Buffer,
    narrow: Option<Buffer>,
    wide: Option<Buffer>,
    object_group: BindGroup,
    styles: Table<StyleData>,
    style_group: BindGroup,
    commands: IndexedCommands,
    command_group: BindGroup,
    object_count: u32,
    section: Vec<SectionObject>,
    visible: Vec<bool>,
    cached_styles: Vec<StyleData>,
    bytes: u64,
    used: u64,
}

impl MeshSlot {
    fn new(device: &Device, queue: &Queue, layouts: &Layouts, mesh: &PackedMesh) -> Self {
        let object_count = mesh.object_count();
        assert!(object_count > 0, "a mesh slot carries an object");
        let object_count = u32::try_from(object_count).expect("a mesh holds u32 objects");
        let mut points = Vec::with_capacity(mesh.vertex_count());
        let mut objects = Vec::with_capacity(object_count as usize);
        let mut section = Vec::with_capacity(object_count as usize);
        for (object, group) in mesh.groups().iter().enumerate() {
            let first = group.first_vertex() as usize;
            let end = first + group.vertex_count() as usize;
            for vertex in &mesh.vertices()[first..end] {
                points.push(PointData {
                    position: [
                        vertex.position()[0],
                        vertex.position()[1],
                        vertex.position()[2],
                        0,
                    ],
                    normal: vertex.normal(),
                    object: object as u32,
                });
            }
            let extent = [0, 1, 2].map(|axis| group.scale()[axis] * UNIT);
            let low = group.min();
            let center = [0, 1, 2].map(|axis| low[axis] + 0.5 * extent[axis]);
            let radius = 0.5 * glam::Vec3::new(extent[0], extent[1], extent[2]).length();
            let wide = group.wide();
            objects.push(ObjectData {
                center_radius: [center[0], center[1], center[2], radius],
                minimum: [low[0], low[1], low[2], 0.0],
                extent: [extent[0], extent[1], extent[2], 0.0],
                narrow_first: if wide { 0 } else { group.first_index() },
                narrow_count: if wide { 0 } else { group.index_count() },
                wide_first: if wide { group.first_index() } else { 0 },
                wide_count: if wide { group.index_count() } else { 0 },
                vertex_base: group.first_vertex(),
                reserved: [0; 3],
            });
            section.push(SectionObject {
                first: group.first_index(),
                count: group.index_count(),
                base: group.first_vertex() as i32,
                wide,
                low,
                high: [0, 1, 2].map(|axis| low[axis] + extent[axis]),
            });
        }
        assert_eq!(
            points.len(),
            mesh.vertex_count(),
            "every packed vertex reaches the gpu"
        );
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.mesh.vertices"),
            contents: bytemuck::cast_slice(&points),
            usage: BufferUsages::VERTEX,
        });
        let narrow = (!mesh.narrow().is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.mesh.indices.narrow"),
                contents: bytemuck::cast_slice(mesh.narrow()),
                usage: BufferUsages::INDEX,
            })
        });
        let wide = (!mesh.wide().is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.mesh.indices.wide"),
                contents: bytemuck::cast_slice(mesh.wide()),
                usage: BufferUsages::INDEX,
            })
        });
        let mut object_table =
            Table::with_capacity(device, "grana.mesh.objects", object_count as usize);
        let grown = object_table.write(device, queue, &objects);
        assert!(!grown, "the object table holds every mesh object");
        let object_group = bind_group(device, &layouts.objects, object_table.buffer());
        let styles = Table::with_capacity(device, "grana.mesh.styles", object_count as usize);
        let style_group = bind_group(device, &layouts.styles, styles.buffer());
        let commands = IndexedCommands::new(
            device,
            "grana.mesh.commands",
            object_count as usize,
            REGIONS,
        );
        let command_group = bind_group(device, &layouts.commands, commands.buffer());
        let bytes = (points.len() * size_of::<PointData>()
            + size_of_val(mesh.narrow())
            + size_of_val(mesh.wide())
            + object_count as usize * size_of::<ObjectData>()
            + object_count as usize * size_of::<StyleData>()) as u64
            + commands.buffer().size();
        Self {
            vertices,
            narrow,
            wide,
            object_group,
            styles,
            style_group,
            commands,
            command_group,
            object_count,
            visible: vec![false; object_count as usize],
            section,
            cached_styles: Vec::new(),
            bytes,
            used: 0,
        }
    }

    fn touch(&mut self, order: u64) {
        self.used = order;
    }

    fn write_styles(&mut self, queue: &Queue, styles: &[Style]) {
        assert_eq!(
            styles.len(),
            self.object_count as usize,
            "every mesh object carries one style"
        );
        let data = styles
            .iter()
            .map(|style| style_data(*style))
            .collect::<Vec<_>>();
        if data != self.cached_styles {
            queue.write_buffer(self.styles.buffer(), 0, bytemuck::cast_slice(&data));
            for (visible, style) in self.visible.iter_mut().zip(styles) {
                *visible = style.visible();
            }
            self.cached_styles = data;
        }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, narrow: usize, wide: usize) {
        pass.set_bind_group(1, &self.object_group, &[]);
        pass.set_bind_group(2, &self.style_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if let Some(indices) = &self.narrow {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint16);
            pass.multi_draw_indexed_indirect(
                self.commands.buffer(),
                self.commands.offset(narrow),
                self.object_count,
            );
        }
        if let Some(indices) = &self.wide {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
            pass.multi_draw_indexed_indirect(
                self.commands.buffer(),
                self.commands.offset(wide),
                self.object_count,
            );
        }
    }

    fn draw_object<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, object: usize) {
        let data = &self.section[object];
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if data.wide {
            let Some(indices) = &self.wide else {
                return;
            };
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        } else {
            let Some(indices) = &self.narrow else {
                return;
            };
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint16);
        }
        pass.draw_indexed(data.first..data.first + data.count, data.base, 0..1);
    }
}

fn cut_stencil(
    compare: CompareFunction,
    pass: StencilOperation,
    write_mask: u32,
) -> DepthStencilState {
    let face = StencilFaceState {
        compare,
        fail_op: StencilOperation::Keep,
        depth_fail_op: StencilOperation::Keep,
        pass_op: pass,
    };
    DepthStencilState {
        format: STENCIL,
        depth_write_enabled: Some(false),
        depth_compare: Some(CompareFunction::Always),
        stencil: StencilState {
            front: face,
            back: face,
            read_mask: 0xFF,
            write_mask,
        },
        bias: DepthBiasState::default(),
    }
}

fn section_axis(section: &SectionData) -> (usize, usize, usize) {
    (
        section.mapping[2] as usize,
        section.mapping[0] as usize,
        section.mapping[1] as usize,
    )
}

fn cut_rect(object: &SectionObject, section: &SectionData, size: [u32; 2]) -> Option<[u32; 4]> {
    let (axis, u, v) = section_axis(section);
    let plane = section.box_min[axis] + section.plane[0] * section.world[axis];
    if object.high[axis] < plane || object.low[axis] > plane {
        return None;
    }
    if section.uv_scale[0] <= 0.0 || section.uv_scale[1] <= 0.0 {
        return None;
    }
    let (first_u, second_u) = cut_span(object.low[u], object.high[u], u, 0, section, size);
    let (first_v, second_v) = cut_span(object.low[v], object.high[v], v, 1, section, size);
    let left = edge(first_u.min(second_u) - SCISSOR_MARGIN, size[0], f32::floor);
    let right = edge(first_u.max(second_u) + SCISSOR_MARGIN, size[0], f32::ceil);
    let top = edge(first_v.min(second_v) - SCISSOR_MARGIN, size[1], f32::floor);
    let bottom = edge(first_v.max(second_v) + SCISSOR_MARGIN, size[1], f32::ceil);
    if right <= left || bottom <= top {
        return None;
    }
    Some([left, top, right - left, bottom - top])
}

fn cut_span(
    low: f32,
    high: f32,
    world_axis: usize,
    pane_axis: usize,
    section: &SectionData,
    size: [u32; 2],
) -> (f32, f32) {
    let length = size[pane_axis] as f32;
    let pixel = |world: f32| {
        let uv = (world - section.box_min[world_axis]) / section.world[world_axis];
        let clip = (uv - section.uv_center[pane_axis]) / section.uv_scale[pane_axis];
        if pane_axis == 0 {
            (clip * 0.5 + 0.5) * length
        } else {
            (0.5 - clip * 0.5) * length
        }
    };
    (pixel(low), pixel(high))
}

fn edge(value: f32, length: u32, round: fn(f32) -> f32) -> u32 {
    (round(value) as i64).clamp(0, length as i64) as u32
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
        array_stride: size_of::<PointData>() as u64,
        step_mode: VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                format: VertexFormat::Unorm16x4,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Unorm16x2,
                offset: 8,
                shader_location: 1,
            },
            VertexAttribute {
                format: VertexFormat::Uint32,
                offset: 12,
                shader_location: 2,
            },
        ],
    }
}
