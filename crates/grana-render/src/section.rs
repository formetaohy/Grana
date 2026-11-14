use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_display::{Plane, srgb_to_linear};
use grana_geometry::{PackedMesh, SectionCutter, SectionIndex, SectionPlane};
use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, Device, FragmentState,
    MultisampleState, PrimitiveState, RenderPass, RenderPipeline, RenderPipelineDescriptor,
    ShaderModuleDescriptor, ShaderSource, VertexState, util::DeviceExt,
};

use crate::layout::Layouts;
use crate::schema::{LayerKey, SceneFrame, Style};
use crate::target::{PICK_FORMAT, SCENE_FORMAT};

const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/section.wgsl")
);

const QUAD_LIMIT: usize = 1 << 16;

const BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 4],
    color: [f32; 4],
    pick: [u32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaneKey {
    normal: [u32; 3],
    right: [u32; 3],
    offset: u32,
}

impl PlaneKey {
    fn of(plane: SectionPlane) -> Self {
        Self {
            normal: plane.normal.map(f32::to_bits),
            right: plane.right.map(f32::to_bits),
            offset: plane.offset.to_bits(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Stamp {
    plane: Option<PlaneKey>,
    layers: Vec<LayerKey>,
    styles: Vec<Style>,
}

struct Indices {
    normal: [u32; 3],
    objects: Vec<SectionIndex>,
}

pub(crate) struct Sections {
    color: RenderPipeline,
    pick: RenderPipeline,
    stamp: Option<Stamp>,
    indices: HashMap<LayerKey, Indices>,
    cutter: SectionCutter,
    vertices: Vec<Vertex>,
    buffer: Option<Buffer>,
    group: Option<BindGroup>,
    count: u32,
}

impl Sections {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.section"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let color = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.section"),
            layout: Some(&layouts.section_pipeline),
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
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BLEND),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let pick = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.section.pick"),
            layout: Some(&layouts.section_pipeline),
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
        Self {
            color,
            pick,
            stamp: None,
            indices: HashMap::new(),
            cutter: SectionCutter::new(),
            vertices: Vec::new(),
            buffer: None,
            group: None,
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.stamp = None;
        self.indices.clear();
        self.vertices.clear();
        self.buffer = None;
        self.group = None;
        self.count = 0;
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
        plane: Option<Plane>,
    ) {
        let plane = plane.map(|plane| {
            let normal = plane.normal();
            SectionPlane::new(normal, normal.any_orthonormal_vector(), plane.offset())
        });
        let key = plane.map(PlaneKey::of);
        let unchanged = self.stamp.as_ref().is_some_and(|stamp| {
            stamp.plane == key
                && stamp.layers.len() == scene.layers.len()
                && stamp
                    .layers
                    .iter()
                    .zip(scene.layers)
                    .all(|(key, layer)| *key == layer.key)
                && stamp.styles == scene.styles
        });
        if unchanged {
            return;
        }
        let vertices = match plane {
            Some(plane) => self.vertices_of(scene, plane),
            None => Vec::new(),
        };
        self.count = u32::try_from(vertices.len()).expect("a section holds u32 vertices");
        self.buffer = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grana.section.vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: BufferUsages::STORAGE,
            })
        });
        self.group = self.buffer.as_ref().map(|buffer| {
            device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.section.group"),
                layout: &layouts.section,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        });
        self.vertices = vertices;
        self.stamp = Some(Stamp {
            plane: key,
            layers: scene.layers.iter().map(|layer| layer.key).collect(),
            styles: scene.styles.to_vec(),
        });
    }

    fn vertices_of(&mut self, scene: &SceneFrame<'_>, plane: SectionPlane) -> Vec<Vertex> {
        let normal = Vec3::from(plane.normal);
        let right = Vec3::from(plane.right);
        let up = Vec3::from(plane.up);
        let origin = normal * plane.offset;
        self.indices
            .retain(|key, _| scene.layers.iter().any(|layer| layer.key == *key));
        let mut vertices = Vec::new();
        for layer in scene.layers {
            let indices = self
                .indices
                .entry(layer.key)
                .and_modify(|indices| {
                    if indices.normal != plane.normal.map(f32::to_bits) {
                        *indices = Indices::new(&layer.mesh, plane.normal);
                    }
                })
                .or_insert_with(|| Indices::new(&layer.mesh, plane.normal));
            let mut quads = Vec::new();
            for (object, index) in indices.objects.iter().enumerate() {
                let style = scene.styles[layer.style_start + object];
                if !style.visible() || style.opacity() <= 0.0 {
                    continue;
                }
                quads.clear();
                self.cutter
                    .cut_indexed(&layer.mesh, object, &plane, index, &mut quads);
                let [red, green, blue] = srgb_to_linear(style.color());
                let color = [red, green, blue, style.opacity()];
                let pick = [layer.style_start as u32 + object as u32 + 1, 0, 0, 0];
                for quad in quads.as_chunks::<4>().0 {
                    let corners = (*quad).map(|point| {
                        let world = origin + right * point[0] + up * point[1];
                        [world.x, world.y, world.z, 1.0]
                    });
                    for corner in [0usize, 1, 2, 0, 2, 3] {
                        vertices.push(Vertex {
                            position: corners[corner],
                            color,
                            pick,
                        });
                    }
                }
            }
        }
        vertices.truncate(QUAD_LIMIT * 6);
        vertices
    }

    pub fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        let (Some(group), pipeline) = (self.group.as_ref(), &self.color) else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..self.count, 0..1);
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        let (Some(group), pipeline) = (self.group.as_ref(), &self.pick) else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..self.count, 0..1);
    }
}

impl Indices {
    fn new(mesh: &PackedMesh, normal: [f32; 3]) -> Self {
        let objects = (0..mesh.groups().len())
            .map(|object| SectionIndex::new(mesh, object, normal))
            .collect();
        Self {
            normal: normal.map(f32::to_bits),
            objects,
        }
    }
}
