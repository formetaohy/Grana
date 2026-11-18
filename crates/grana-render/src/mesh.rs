use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use grana_geometry::PackedMesh;
use grana_gpu::Pipelines;
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
use crate::target::{NORMAL_FORMAT, PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;
use grana_display::{Shading, srgb_to_linear};

const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_cull.wgsl")
);
const NORMAL_SHADER: &str = concat!(
    include_str!("shader/mesh_binding.wgsl"),
    "\n",
    include_str!("shader/mesh_binding_objects.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/mesh_common.wgsl"),
    "\n",
    include_str!("shader/mesh_vertex.wgsl"),
    "\n",
    include_str!("shader/mesh_normal.wgsl")
);
const FORWARD_SHADER: &str = concat!(
    include_str!("shader/mesh_binding.wgsl"),
    "\n",
    include_str!("shader/mesh_binding_objects.wgsl"),
    "\n",
    include_str!("shader/octahedral.wgsl"),
    "\n",
    include_str!("shader/brdf.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/oit_key.wgsl"),
    "\n",
    include_str!("shader/oit.wgsl"),
    "\n",
    include_str!("shader/mesh_common.wgsl"),
    "\n",
    include_str!("shader/mesh_shade.wgsl"),
    "\n",
    include_str!("shader/mesh_vertex.wgsl"),
    "\n",
    include_str!("shader/mesh_shadow.wgsl"),
    "\n",
    include_str!("shader/mesh_forward.wgsl")
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
    surface: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MeshUniforms {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
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
    forward: [f32; 4],
    projection: [f32; 4],
    lens: [f32; 4],
}

const _: () = {
    assert!(size_of::<PointData>() == 16);
    assert!(size_of::<ObjectData>() == 80);
    assert!(size_of::<StyleData>() == 48);
    assert!(size_of::<MeshUniforms>() == 448);
};

pub(crate) struct MeshPasses {
    slots: HashMap<LayerKey, MeshSlot>,
    active: Vec<LayerKey>,
    order: u64,
    budget: u64,
    resident: u64,
    cull: ComputePipeline,
    shadow_cull: ComputePipeline,
    normal_pipeline: RenderPipeline,
    opaque_pipeline: RenderPipeline,
    oit_pipeline: RenderPipeline,
    pick_pipeline: RenderPipeline,
    shadow_pipeline: RenderPipeline,
    uniforms: Buffer,
    uniform_group: BindGroup,
}

impl MeshPasses {
    pub fn new(device: &Device, layouts: &Layouts, budget: u64, pipelines: &Pipelines) -> Self {
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
        let oit_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.oit.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
                Some(&layouts.shadow),
                Some(&layouts.oit_write),
                None,
                None,
                Some(&layouts.ao_read),
            ],
            immediate_size: 0,
        });
        let normal_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.normal.pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&layouts.objects)],
            immediate_size: 0,
        });
        let opaque_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.mesh.opaque.pipeline_layout"),
            bind_group_layouts: &[
                Some(&uniform_layout),
                Some(&layouts.objects),
                Some(&layouts.styles),
                Some(&layouts.shadow),
                None,
                None,
                None,
                Some(&layouts.ao_read),
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
            cache: pipelines.cache(),
        });
        let shadow_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.cull.shadow"),
            layout: Some(&layouts.mesh_cull),
            module: &cull_module,
            entry_point: Some("mesh_cull_shadow"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        let normal_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.normal"),
            source: ShaderSource::Wgsl(NORMAL_SHADER.into()),
        });
        let forward_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.forward"),
            source: ShaderSource::Wgsl(FORWARD_SHADER.into()),
        });
        let normal_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.normal"),
            layout: Some(&normal_layout),
            vertex: VertexState {
                module: &normal_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &normal_module,
                entry_point: Some("fs_normal"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: NORMAL_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let opaque_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.opaque"),
            layout: Some(&opaque_layout),
            vertex: VertexState {
                module: &forward_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::LessEqual)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &forward_module,
                entry_point: Some("fs_opaque"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let shadow_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.shadow"),
            layout: Some(&shadow_layout),
            vertex: VertexState {
                module: &forward_module,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let oit_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.oit"),
            layout: Some(&oit_layout),
            vertex: VertexState {
                module: &forward_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &forward_module,
                entry_point: Some("fs_oit"),
                compilation_options: Default::default(),
                targets: &crate::oit::residual_targets(),
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let pick_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.mesh.pick"),
            layout: Some(&pick_layout),
            vertex: VertexState {
                module: &forward_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &forward_module,
                entry_point: Some("fs_pick"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: PICK_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
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
            normal_pipeline,
            opaque_pipeline,
            oit_pipeline,
            pick_pipeline,
            shadow_pipeline,
            uniforms,
            uniform_group,
        }
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
                layer.objects(),
                layer.mesh.object_count(),
                "every mesh object carries one style"
            );
            assert!(keys.insert(layer.key), "a scene names each mesh layer once");
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
            slot.write_styles(queue, &layer.styles);
            slot.write_pick(device, queue, layer.pick_base);
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
            inverse_view_projection: frame.inv_view_projection,
            eye: frame.eye,
            light: frame.light,
            shading: [shading.ambient, shading.diffuse, 0.0, 0.0],
            region_center: frame.region_center,
            region_axis: frame.region_axis,
            jitter: frame.jitter,
            light_view: frame.light_view,
            light_reach: frame.light_reach,
            shadow: [shading.shadows, 0.0, 0.0, 0.0],
            viewport: frame.viewport,
            oit: frame.oit,
            clip: frame.clip,
            forward: frame.forward,
            projection: frame.projection,
            lens: frame.lens,
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

    pub fn draw_normal(
        &self,
        encoder: &mut CommandEncoder,
        region: [u32; 4],
        normal: &crate::target::NormalTarget,
        depth: &TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.normal.pass"),
            color_attachments: &[Some(load(normal.view()))],
            depth_stencil_attachment: Some(depth_attachment(depth)),
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(&self.normal_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for key in &self.active {
            self.slots[key].draw_normal(&mut pass);
        }
    }

    pub fn draw_opaque(
        &self,
        encoder: &mut CommandEncoder,
        region: [u32; 4],
        color: &TextureView,
        depth: &TextureView,
        shadow: &BindGroup,
        ao: &BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.opaque.pass"),
            color_attachments: &[Some(load(color))],
            depth_stencil_attachment: Some(depth_attachment(depth)),
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        pass.set_pipeline(&self.opaque_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(7, ao, &[]);
        for key in &self.active {
            self.slots[key].draw(&mut pass, OPAQUE_NARROW, OPAQUE_WIDE);
        }
    }

    pub fn draw_shadow(&self, encoder: &mut CommandEncoder, target: &TextureView) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.mesh.shadow.pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(depth_attachment(target)),
            ..Default::default()
        });
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for key in &self.active {
            self.slots[key].draw(&mut pass, SHADOW_NARROW, SHADOW_WIDE);
        }
    }

    pub fn draw_oit<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        ao: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.oit_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(7, ao, &[]);
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
    cached_styles: Vec<StyleData>,
    pick_base: Option<u32>,
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
        if self.cached_styles.len() != styles.len() {
            self.cached_styles = vec![StyleData::zeroed(); styles.len()];
        }
        let mut first = usize::MAX;
        let mut end = 0;
        for (row, style) in styles.iter().enumerate() {
            let data = style_data(*style);
            if self.cached_styles[row] != data {
                self.cached_styles[row] = data;
                first = first.min(row);
                end = row + 1;
            }
        }
        if first < end {
            queue.write_buffer(
                self.styles.buffer(),
                (first * size_of::<StyleData>()) as u64,
                bytemuck::cast_slice(&self.cached_styles[first..end]),
            );
        }
    }

    fn write_pick(&mut self, device: &Device, queue: &Queue, pick_base: u32) {
        if self.pick_base == Some(pick_base) {
            return;
        }
        let first = pick_base.checked_add(1).expect("a style row fits u32");
        let rows = (0..self.object_count)
            .map(|index| first + index)
            .collect::<Vec<_>>();
        let grown = self.pick.write(device, queue, &rows);
        assert!(!grown, "the pick table holds every mesh object");
        self.pick_base = Some(pick_base);
    }

    fn draw_normal<'a>(&'a self, pass: &mut RenderPass<'a>) {
        pass.set_bind_group(1, &self.object_group, &[]);
        self.draw_regions(pass, OPAQUE_NARROW, OPAQUE_WIDE);
    }

    fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>) {
        pass.set_bind_group(1, &self.object_group, &[]);
        pass.set_bind_group(5, &self.pick_group, &[]);
        self.draw_regions(pass, OPAQUE_NARROW, OPAQUE_WIDE);
        self.draw_regions(pass, LAYER_NARROW, LAYER_WIDE);
    }

    fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, narrow: usize, wide: usize) {
        pass.set_bind_group(1, &self.object_group, &[]);
        pass.set_bind_group(2, &self.style_group, &[]);
        self.draw_regions(pass, narrow, wide);
    }

    fn draw_regions<'a>(&'a self, pass: &mut RenderPass<'a>, narrow: usize, wide: usize) {
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
        surface: [style.roughness(), style.metallic(), 0.0, 0.0],
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

fn load(view: &TextureView) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    }
}

fn depth_attachment(view: &TextureView) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        }),
        stencil_ops: None,
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
