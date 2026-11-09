use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use grana_geometry::PackedMesh;
use grana_gpu::{IndexedCommands, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, ComputePipeline,
    ComputePipelineDescriptor, Device, FragmentState, IndexFormat, LoadOp, MultisampleState,
    Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
    util::DeviceExt,
};

use crate::layout::Layouts;
use crate::schema::{LayerKey, SceneFrame, Style};
use crate::target::{PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;
use grana_display::{Shading, srgb_to_linear};

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
    include_str!("shader/oit.wgsl"),
    "\n",
    include_str!("shader/mesh.wgsl")
);

const OPAQUE_NARROW: usize = 0;
const OPAQUE_WIDE: usize = 1;
const LAYER_NARROW: usize = 2;
const LAYER_WIDE: usize = 3;
const SHADOW_NARROW: usize = 4;
const SHADOW_WIDE: usize = 5;
const REGIONS: usize = 6;
const UNIT: f32 = 65535.0;
const WORKGROUP: u32 = 64;

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
pub(crate) struct StyleData {
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
    region_center: [f32; 4],
    region_axis: [[f32; 4]; 3],
    jitter: [f32; 4],
    light_view: [[f32; 4]; 4],
    light_reach: [f32; 4],
    shadow: [f32; 4],
    viewport: [f32; 4],
    oit: [u32; 4],
    clip: [f32; 4],
}

const _: () = {
    assert!(size_of::<PointData>() == 16);
    assert!(size_of::<ObjectData>() == 80);
    assert!(size_of::<StyleData>() == 32);
    assert!(size_of::<MeshUniforms>() == 336);
};

pub(crate) struct MeshPasses {
    slots: HashMap<LayerKey, MeshSlot>,
    active: Vec<LayerKey>,
    order: u64,
    budget: u64,
    resident: u64,
    cull: ComputePipeline,
    shadow_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    oit_pipeline: RenderPipeline,
    tail_pipeline: RenderPipeline,
    color_pipeline: RenderPipeline,
    pick_pipeline: RenderPipeline,
    shadow_pipeline: RenderPipeline,
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
        let oit_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.oit.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
                Some(&layouts.shadow),
                Some(&layouts.oit_write),
            ],
            immediate_size: 0,
        });
        let tail_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.tail.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
                Some(&layouts.shadow),
                Some(&layouts.oit_write),
                None,
                Some(&layouts.oit_coverage),
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
        let oit_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.oit"),
            layout: Some(&oit_layout),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_module,
                entry_point: Some("fs_oit_key"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: crate::oit::COVERAGE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let tail_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.tail"),
            layout: Some(&tail_layout),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_module,
                entry_point: Some("fs_oit_tail"),
                compilation_options: Default::default(),
                targets: &crate::oit::tail_targets(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let color_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.oit.color"),
            layout: Some(&tail_layout),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::Always)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_module,
                entry_point: Some("fs_oit_color"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pick_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.pick.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                None,
                None,
                None,
                Some(&layouts.pick),
            ],
            immediate_size: 0,
        });
        let pick_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.pick"),
            layout: Some(&pick_layout),
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
                entry_point: Some("fs_pick"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: PICK_FORMAT,
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
            slots: HashMap::new(),
            active: Vec::new(),
            order: 0,
            budget,
            resident: 0,
            cull,
            shadow_cull,
            volume_pipeline,
            oit_pipeline,
            tail_pipeline,
            color_pipeline,
            pick_pipeline,
            shadow_pipeline,
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
                let slot = MeshSlot::new(device, queue, layouts, &layer.mesh);
                self.resident += slot.bytes;
                self.slots.insert(layer.key, slot);
            }
            let slot = self
                .slots
                .get_mut(&layer.key)
                .expect("the mesh slot exists");
            slot.touch(order);
            slot.style_start = layer.style_start as u32;
            slot.write_styles(
                queue,
                &scene.styles[layer.style_start..layer.style_start + layer.style_count],
            );
            slot.write_pick(device, queue, layer.style_start);
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
            region_center: frame.region_center,
            region_axis: frame.region_axis,
            jitter: frame.jitter,
            light_view: frame.light_view,
            light_reach: frame.light_reach,
            shadow: [shading.shadows, 0.0, 0.0, 0.0],
            viewport: frame.viewport,
            oit: frame.oit,
            clip: frame.clip,
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
            self.slots[key].draw(&mut pass, OPAQUE_NARROW, OPAQUE_WIDE);
        }
    }

    pub fn draw_oit<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.oit_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        for key in &self.active {
            self.slots[key].draw(pass, LAYER_NARROW, LAYER_WIDE);
        }
    }

    pub fn draw_color<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        coverage: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.color_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(6, coverage, &[]);
        for key in &self.active {
            self.slots[key].draw(pass, LAYER_NARROW, LAYER_WIDE);
        }
    }

    pub fn draw_tail<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        coverage: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.tail_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(6, coverage, &[]);
        for key in &self.active {
            self.slots[key].draw(pass, LAYER_NARROW, LAYER_WIDE);
        }
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pick_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for key in &self.active {
            self.slots[key].draw_pick(pass);
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
    pick: Table<u32>,
    pick_group: BindGroup,
    commands: IndexedCommands,
    command_group: BindGroup,
    object_count: u32,
    style_start: u32,
    cached_styles: Vec<StyleData>,
    pick_base: Option<usize>,
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
        let pick = Table::with_capacity(device, "grana.mesh.pick", object_count as usize);
        let pick_group = bind_group(device, &layouts.pick, pick.buffer());
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
            + object_count as usize * size_of::<StyleData>()
            + object_count as usize * size_of::<u32>()) as u64
            + commands.buffer().size();
        Self {
            vertices,
            narrow,
            wide,
            object_group,
            styles,
            style_group,
            pick,
            pick_group,
            commands,
            command_group,
            object_count,
            style_start: 0,
            cached_styles: Vec::new(),
            pick_base: None,
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
            self.cached_styles = data;
        }
    }

    fn write_pick(&mut self, device: &Device, queue: &Queue, style_start: usize) {
        if self.pick_base == Some(style_start) {
            return;
        }
        let base = u32::try_from(style_start + 1).expect("a style row fits u32");
        let rows = (0..self.object_count)
            .map(|index| base + index)
            .collect::<Vec<_>>();
        let grown = self.pick.write(device, queue, &rows);
        assert!(!grown, "the pick table holds every mesh object");
        self.pick_base = Some(style_start);
    }

    fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>) {
        pass.set_bind_group(1, &self.object_group, &[]);
        pass.set_bind_group(5, &self.pick_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if let Some(indices) = &self.narrow {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint16);
            for region in [OPAQUE_NARROW, LAYER_NARROW] {
                pass.multi_draw_indexed_indirect(
                    self.commands.buffer(),
                    self.commands.offset(region),
                    self.object_count,
                );
            }
        }
        if let Some(indices) = &self.wide {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
            for region in [OPAQUE_WIDE, LAYER_WIDE] {
                pass.multi_draw_indexed_indirect(
                    self.commands.buffer(),
                    self.commands.offset(region),
                    self.object_count,
                );
            }
        }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, narrow: usize, wide: usize) -> u32 {
        pass.set_bind_group(1, &self.object_group, &[]);
        pass.set_bind_group(2, &self.style_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        let mut draws = 0;
        if let Some(indices) = &self.narrow {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint16);
            pass.multi_draw_indexed_indirect(
                self.commands.buffer(),
                self.commands.offset(narrow),
                self.object_count,
            );
            draws += 1;
        }
        if let Some(indices) = &self.wide {
            pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
            pass.multi_draw_indexed_indirect(
                self.commands.buffer(),
                self.commands.offset(wide),
                self.object_count,
            );
            draws += 1;
        }
        draws
    }
}

pub(crate) fn style_data(style: Style) -> StyleData {
    let [red, green, blue] = srgb_to_linear(style.color());
    StyleData {
        color: [red, green, blue, style.opacity()],
        flags: [
            if style.visible() { 1.0 } else { 0.0 },
            if style.selected() { 1.0 } else { 0.0 },
            if style.transparent() { 0.0 } else { 1.0 },
            0.0,
        ],
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
