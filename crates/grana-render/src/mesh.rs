use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use grana_geometry::PackedMesh;
use grana_gpu::Pipelines;
use grana_gpu::{IndirectDraw, Table};
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
use crate::schema::{Layer, LayerKey, SceneFrame, Style};
use crate::target::{NORMAL_FORMAT, PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;
use grana_display::{Shading, srgb_to_linear};

const COMPACT_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/mesh_compact.wgsl")
);
const ARGS_SHADER: &str = include_str!("shader/mesh_args.wgsl");
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
const OPAQUE: usize = 0;
const LAYER: usize = 1;
const PICK: usize = 2;
const SHADOW: usize = 3;
const REGIONS: usize = 4;
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
    first_index: u32,
    index_count: u32,
    reserved: [u32; 2],
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
    assert!(size_of::<ObjectData>() == 64);
    assert!(size_of::<StyleData>() == 48);
    assert!(size_of::<MeshUniforms>() == 448);
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GeometryKey {
    owner: u64,
    part: u32,
    geometry: u64,
    detail: u16,
}

impl GeometryKey {
    fn of(layer: &Layer) -> Self {
        Self {
            owner: layer.key.owner(),
            part: layer.key.part(),
            geometry: layer.geometry,
            detail: layer.key.detail(),
        }
    }
}

pub(crate) struct MeshPasses {
    geometries: HashMap<GeometryKey, MeshGeometry>,
    rows: HashMap<LayerKey, MeshRows>,
    active: Vec<(GeometryKey, LayerKey)>,
    order: u64,
    budget: u64,
    resident: u64,
    compact: ComputePipeline,
    settle: ComputePipeline,
    count: ComputePipeline,
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
        let compact_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.compact"),
            source: ShaderSource::Wgsl(COMPACT_SHADER.into()),
        });
        let count = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.count"),
            layout: Some(&layouts.mesh_compact_pipeline),
            module: &compact_module,
            entry_point: Some("mesh_count"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        let compact = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.compact"),
            layout: Some(&layouts.mesh_compact_pipeline),
            module: &compact_module,
            entry_point: Some("mesh_compact"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        let args_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.mesh.args"),
            source: ShaderSource::Wgsl(ARGS_SHADER.into()),
        });
        let settle = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.mesh.args"),
            layout: Some(&layouts.mesh_args_pipeline),
            module: &args_module,
            entry_point: Some("mesh_args"),
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
            geometries: HashMap::new(),
            rows: HashMap::new(),
            active: Vec::new(),
            order: 0,
            budget,
            resident: 0,
            compact,
            settle,
            count,
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
        self.geometries.clear();
        self.rows.clear();
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
            let geometry_key = GeometryKey::of(layer);
            let resident = self
                .geometries
                .get(&geometry_key)
                .is_some_and(|geometry| geometry.holds(&layer.mesh));
            if !resident {
                let geometry = MeshGeometry::new(device, queue, layouts, &layer.mesh);
                self.resident += geometry.bytes;
                if let Some(held) = self.geometries.insert(geometry_key, geometry) {
                    self.resident = self.resident.saturating_sub(held.bytes);
                }
            }
            let geometry = self
                .geometries
                .get_mut(&geometry_key)
                .expect("the mesh geometry is resident");
            geometry.touch(order);
            let fitted = self
                .rows
                .get(&layer.key)
                .is_some_and(|rows| rows.holds(layer));
            if !fitted {
                let rows = MeshRows::new(device, layouts, layer.objects(), layer.geometry);
                self.resident += rows.bytes;
                if let Some(held) = self.rows.insert(layer.key, rows) {
                    self.resident = self.resident.saturating_sub(held.bytes);
                }
            }
            let rows = self
                .rows
                .get_mut(&layer.key)
                .expect("the mesh rows are resident");
            rows.touch(order);
            rows.write_styles(queue, &layer.styles);
            rows.write_pick(device, queue, layer.pick_base);
            self.active.push((geometry_key, layer.key));
        }
        self.evict();
    }

    fn evict(&mut self) {
        while self.resident > self.budget {
            let held = self
                .geometries
                .iter()
                .filter(|(key, _)| !self.active.iter().any(|active| active.0 == **key))
                .min_by_key(|(_, geometry)| geometry.used)
                .map(|(key, _)| *key);
            if let Some(key) = held {
                if let Some(geometry) = self.geometries.remove(&key) {
                    self.resident = self.resident.saturating_sub(geometry.bytes);
                }
                continue;
            }
            let held = self
                .rows
                .iter()
                .filter(|(key, _)| !self.active.iter().any(|active| active.1 == **key))
                .min_by_key(|(_, rows)| rows.used)
                .map(|(key, _)| *key);
            let Some(key) = held else {
                break;
            };
            if let Some(rows) = self.rows.remove(&key) {
                self.resident = self.resident.saturating_sub(rows.bytes);
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

    pub fn compact(&self, queue: &Queue, encoder: &mut CommandEncoder, scene: &BindGroup) {
        if self.active.is_empty() {
            return;
        }
        for (geometry, _) in &self.active {
            self.geometries[geometry].draws.reset(queue);
        }
        for (pipeline, label) in [
            (&self.count, "grana.mesh.count.pass"),
            (&self.compact, "grana.mesh.compact.pass"),
        ] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, scene, &[]);
            for (geometry, rows) in &self.active {
                let geometry = &self.geometries[geometry];
                let rows = &self.rows[rows];
                pass.set_bind_group(1, &geometry.object_group, &[]);
                pass.set_bind_group(2, &rows.style_group, &[]);
                pass.set_bind_group(3, &geometry.compact_group, &[]);
                pass.dispatch_workgroups(geometry.object_count.div_ceil(WORKGROUP), 1, 1);
            }
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.mesh.args.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.settle);
        for (geometry, _) in &self.active {
            let geometry = &self.geometries[geometry];
            pass.set_bind_group(0, &geometry.args_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
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
        for (geometry, _) in &self.active {
            let geometry = &self.geometries[geometry];
            pass.set_bind_group(1, &geometry.object_group, &[]);
            geometry.draw(&mut pass, OPAQUE);
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
        for (geometry, rows) in &self.active {
            self.shade(&mut pass, geometry, rows, OPAQUE);
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
        for (geometry, rows) in &self.active {
            self.shade(&mut pass, geometry, rows, SHADOW);
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
        for (geometry, rows) in &self.active {
            self.shade(pass, geometry, rows, LAYER);
        }
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pick_pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        for (geometry, rows) in &self.active {
            let geometry = &self.geometries[geometry];
            let rows = &self.rows[rows];
            pass.set_bind_group(1, &geometry.object_group, &[]);
            pass.set_bind_group(5, &rows.pick_group, &[]);
            geometry.draw(pass, PICK);
        }
    }

    fn shade<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        geometry: &'a GeometryKey,
        rows: &'a LayerKey,
        region: usize,
    ) {
        let geometry = &self.geometries[geometry];
        let rows = &self.rows[rows];
        pass.set_bind_group(1, &geometry.object_group, &[]);
        pass.set_bind_group(2, &rows.style_group, &[]);
        geometry.draw(pass, region);
    }
}

struct MeshGeometry {
    vertices: Buffer,
    pool: Buffer,
    object_group: BindGroup,
    compact_group: BindGroup,
    args_group: BindGroup,
    draws: IndirectDraw,
    object_count: u32,
    vertex_count: usize,
    index_count: usize,
    bytes: u64,
    used: u64,
}

impl MeshGeometry {
    fn new(device: &Device, queue: &Queue, layouts: &Layouts, mesh: &PackedMesh) -> Self {
        let object_count = mesh.object_count();
        assert!(object_count > 0, "a mesh slot carries an object");
        let object_count = u32::try_from(object_count).expect("a mesh holds u32 objects");
        let mut points = Vec::with_capacity(mesh.vertex_count());
        let mut objects = Vec::with_capacity(object_count as usize);
        let mut indices = Vec::with_capacity(mesh.narrow().len() + mesh.wide().len());
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
            let base = group.first_vertex();
            let first_index = indices.len() as u32;
            let start = group.first_index() as usize;
            let stop = start + group.index_count() as usize;
            if group.wide() {
                indices.extend(mesh.wide()[start..stop].iter().map(|index| index + base));
            } else {
                indices.extend(
                    mesh.narrow()[start..stop]
                        .iter()
                        .map(|index| u32::from(*index) + base),
                );
            }
            objects.push(ObjectData {
                center_radius: [center[0], center[1], center[2], radius],
                minimum: [low[0], low[1], low[2], 0.0],
                extent: [extent[0], extent[1], extent[2], 0.0],
                first_index,
                index_count: group.index_count(),
                reserved: [0; 2],
            });
        }
        assert_eq!(
            points.len(),
            mesh.vertex_count(),
            "every packed vertex reaches the gpu"
        );
        let index_count = mesh.narrow().len() + mesh.wide().len();
        assert_eq!(
            indices.len(),
            index_count,
            "every packed index reaches the gpu"
        );
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.mesh.vertices"),
            contents: bytemuck::cast_slice(&points),
            usage: BufferUsages::VERTEX,
        });
        let indices = if index_count > 0 {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.mesh.indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: BufferUsages::STORAGE,
            })
        } else {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grana.mesh.indices"),
                size: size_of::<u32>() as u64,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let pool = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.mesh.pool"),
            size: (index_count * size_of::<u32>()).max(size_of::<u32>()) as u64,
            usage: BufferUsages::INDEX | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mut object_table =
            Table::with_capacity(device, "grana.mesh.objects", object_count as usize);
        let grown = object_table.write(device, queue, &objects);
        assert!(!grown, "the object table holds every mesh object");
        let object_group = bind_group(device, &layouts.objects, object_table.buffer());
        let draws = IndirectDraw::new(device, "grana.mesh.args", REGIONS);
        let compact_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.mesh.compact.group"),
            layout: &layouts.mesh_compact,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: indices.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: pool.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: draws.counters().as_entire_binding(),
                },
            ],
        });
        let args_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.mesh.args.group"),
            layout: &layouts.mesh_args,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: draws.counters().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: draws.args().as_entire_binding(),
                },
            ],
        });
        let bytes = (points.len() * size_of::<PointData>()
            + indices.size() as usize
            + pool.size() as usize
            + object_count as usize * size_of::<ObjectData>()) as u64
            + draws.args().size()
            + draws.counters().size();
        Self {
            vertices,
            pool,
            object_group,
            compact_group,
            args_group,
            draws,
            object_count,
            vertex_count: mesh.vertex_count(),
            index_count,
            bytes,
            used: 0,
        }
    }

    fn holds(&self, mesh: &PackedMesh) -> bool {
        self.object_count as usize == mesh.object_count()
            && self.vertex_count == mesh.vertex_count()
            && self.index_count == mesh.narrow().len() + mesh.wide().len()
    }

    fn touch(&mut self, order: u64) {
        self.used = order;
    }

    fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, region: usize) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.pool.slice(..), IndexFormat::Uint32);
        pass.draw_indexed_indirect(self.draws.args(), self.draws.region(region));
    }
}

struct MeshRows {
    objects: usize,
    geometry: u64,
    styles: Table<StyleData>,
    style_group: BindGroup,
    pick: Table<u32>,
    pick_group: BindGroup,
    cached_styles: Vec<StyleData>,
    pick_base: Option<u32>,
    bytes: u64,
    used: u64,
}

impl MeshRows {
    fn new(device: &Device, layouts: &Layouts, objects: usize, geometry: u64) -> Self {
        assert!(objects > 0, "a mesh layer carries an object");
        let styles = Table::with_capacity(device, "grana.mesh.styles", objects);
        let style_group = bind_group(device, &layouts.styles, styles.buffer());
        let pick = Table::with_capacity(device, "grana.mesh.pick", objects);
        let pick_group = bind_group(device, &layouts.pick, pick.buffer());
        let bytes = (objects * size_of::<StyleData>() + objects * size_of::<u32>()) as u64;
        Self {
            objects,
            geometry,
            styles,
            style_group,
            pick,
            pick_group,
            cached_styles: Vec::new(),
            pick_base: None,
            bytes,
            used: 0,
        }
    }

    fn holds(&self, layer: &Layer) -> bool {
        self.objects == layer.objects() && self.geometry == layer.geometry
    }

    fn touch(&mut self, order: u64) {
        self.used = order;
    }

    fn write_styles(&mut self, queue: &Queue, styles: &[Style]) {
        assert_eq!(
            styles.len(),
            self.objects,
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
        let rows = (0..self.objects)
            .map(|index| first + index as u32)
            .collect::<Vec<_>>();
        let grown = self.pick.write(device, queue, &rows);
        assert!(!grown, "the pick table holds every mesh object");
        self.pick_base = Some(pick_base);
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
