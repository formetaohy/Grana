use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_display::{Plane, srgb_to_linear};
use grana_geometry::{PackedMesh, SectionCutter, SectionIndex, SectionPlane};
use grana_gpu::{Pipelines, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendComponent, BlendFactor, BlendOperation,
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, Device, FragmentState,
    IndexFormat, MultisampleState, PrimitiveState, Queue, RenderPass, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, VertexState, util::DeviceExt,
};

use crate::layout::Layouts;
use crate::schema::{Digest, LayerKey, SceneFrame, Style};
use crate::target::{PICK_FORMAT, SCENE_FORMAT};

const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/section.wgsl")
);

const CHUNK_QUADS: usize = 1 << 16;
const INDICES: [usize; 6] = [0, 1, 2, 0, 2, 3];

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
struct SectionStyle {
    color: [f32; 4],
    flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct SectionPlaneData {
    right: [f32; 4],
    up: [f32; 4],
    origin: [f32; 4],
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

struct Indices {
    normal: [u32; 3],
    objects: Vec<SectionIndex>,
}

struct Chunk {
    corners: Table<[f32; 2]>,
    styles: Table<u32>,
    group: Option<BindGroup>,
    quads: u32,
}

impl Chunk {
    fn new(device: &Device) -> Self {
        Self {
            corners: Table::new(device, "grana.section.corners"),
            styles: Table::new(device, "grana.section.quads"),
            group: None,
            quads: 0,
        }
    }

    fn write(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        corners: &[[f32; 2]],
        styles: &[u32],
    ) {
        let grown = self.corners.write(device, queue, corners);
        let grown = self.styles.write(device, queue, styles) || grown;
        if grown || self.group.is_none() {
            self.group = Some(group(
                device,
                layouts,
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: self.corners.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: self.styles.buffer().as_entire_binding(),
                    },
                ],
            ));
        }
        self.quads = u32::try_from(styles.len()).expect("a section chunk holds u32 quads");
    }
}

struct StyleTable {
    styles: Table<SectionStyle>,
    plane: Buffer,
    group: Option<BindGroup>,
}

impl StyleTable {
    fn new(device: &Device) -> Self {
        Self {
            styles: Table::new(device, "grana.section.styles"),
            plane: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grana.section.plane"),
                size: size_of::<SectionPlaneData>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            group: None,
        }
    }

    fn write(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        styles: &[SectionStyle],
    ) {
        let grown = self.styles.write(device, queue, styles);
        if grown || self.group.is_none() {
            self.group = Some(style_group(device, layouts, self));
        }
    }

    fn write_plane(&self, queue: &Queue, plane: SectionPlaneData) {
        queue.write_buffer(&self.plane, 0, bytemuck::bytes_of(&plane));
    }

    fn group(&self) -> Option<&BindGroup> {
        self.group.as_ref()
    }
}

pub(crate) struct Sections {
    color: RenderPipeline,
    pick: RenderPipeline,
    geometry: Option<u64>,
    styles: Option<u64>,
    indices: HashMap<LayerKey, Indices>,
    cutter: SectionCutter,
    chunks: Vec<Chunk>,
    index: Buffer,
    style: StyleTable,
    corners: Vec<[f32; 2]>,
    quad_styles: Vec<u32>,
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
        let indices = (0..CHUNK_QUADS * 6)
            .map(|index| (index / 6 * 4 + INDICES[index % 6]) as u32)
            .collect::<Vec<_>>();
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grana.section.indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: BufferUsages::INDEX,
        });
        Self {
            color,
            pick,
            geometry: None,
            styles: None,
            indices: HashMap::new(),
            cutter: SectionCutter::new(),
            chunks: Vec::new(),
            index,
            style: StyleTable::new(device),
            corners: Vec::new(),
            quad_styles: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.geometry = None;
        self.styles = None;
        self.indices.clear();
        for chunk in &mut self.chunks {
            chunk.quads = 0;
        }
        self.corners.clear();
        self.quad_styles.clear();
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
        plane: Option<Plane>,
    ) {
        let plane = plane.map(|plane| {
            let normal = plane.normal();
            SectionPlane::new(normal, normal.any_orthonormal_vector(), plane.offset())
        });
        let key = plane.map(PlaneKey::of);
        let geometry = geometry_stamp(scene, key);
        if self.geometry != Some(geometry) {
            self.geometry = Some(geometry);
            self.corners.clear();
            self.quad_styles.clear();
            if let Some(plane) = plane {
                self.cut(scene, plane);
            }
            self.write_geometry(device, queue, layouts);
        }
        let Some(plane) = plane else {
            return;
        };
        let origin = Vec3::from(plane.normal) * plane.offset;
        self.style.write_plane(
            queue,
            SectionPlaneData {
                right: Vec3::from(plane.right).extend(0.0).to_array(),
                up: Vec3::from(plane.up).extend(0.0).to_array(),
                origin: origin.extend(0.0).to_array(),
            },
        );
        let styles = style_stamp(scene.styles);
        if self.styles != Some(styles) {
            self.styles = Some(styles);
            let rows = scene
                .styles
                .iter()
                .map(|style| {
                    let [red, green, blue] = srgb_to_linear(style.color());
                    SectionStyle {
                        color: [red, green, blue, style.opacity()],
                        flags: [f32::from(u8::from(style.visible())), 0.0, 0.0, 0.0],
                    }
                })
                .collect::<Vec<_>>();
            self.style.write(device, queue, layouts, &rows);
        }
    }

    fn cut(&mut self, scene: &SceneFrame<'_>, plane: SectionPlane) {
        self.indices
            .retain(|key, _| scene.layers.iter().any(|layer| layer.key == *key));
        let mut quads = Vec::new();
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
            for (object, index) in indices.objects.iter().enumerate() {
                let style_index = layer.style_start + object;
                if !scene.styles[style_index].visible() {
                    continue;
                }
                quads.clear();
                self.cutter
                    .cut_indexed(&layer.mesh, object, &plane, index, &mut quads);
                for quad in quads.as_chunks::<4>().0 {
                    self.corners.extend_from_slice(quad);
                    self.quad_styles.push(style_index as u32);
                }
            }
        }
    }

    fn write_geometry(&mut self, device: &Device, queue: &Queue, layouts: &Layouts) {
        let quads = self.quad_styles.len();
        let chunks = quads.div_ceil(CHUNK_QUADS);
        while self.chunks.len() < chunks {
            self.chunks.push(Chunk::new(device));
        }
        for (index, chunk) in self.chunks.iter_mut().enumerate() {
            let start = index * CHUNK_QUADS;
            let end = ((index + 1) * CHUNK_QUADS).min(quads);
            if start >= end {
                chunk.quads = 0;
                continue;
            }
            chunk.write(
                device,
                queue,
                layouts,
                &self.corners[start * 4..end * 4],
                &self.quad_styles[start..end],
            );
        }
    }

    pub fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        let Some(styles) = self.style.group() else {
            return;
        };
        pass.set_pipeline(&self.color);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(2, styles, &[]);
        pass.set_index_buffer(self.index.slice(..), IndexFormat::Uint32);
        for chunk in &self.chunks {
            let Some(group) = chunk.group.as_ref() else {
                continue;
            };
            if chunk.quads == 0 {
                continue;
            }
            pass.set_bind_group(1, group, &[]);
            pass.draw_indexed(0..chunk.quads * 6, 0, 0..1);
        }
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        let Some(styles) = self.style.group() else {
            return;
        };
        pass.set_pipeline(&self.pick);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(2, styles, &[]);
        pass.set_index_buffer(self.index.slice(..), IndexFormat::Uint32);
        for chunk in &self.chunks {
            let Some(group) = chunk.group.as_ref() else {
                continue;
            };
            if chunk.quads == 0 {
                continue;
            }
            pass.set_bind_group(1, group, &[]);
            pass.draw_indexed(0..chunk.quads * 6, 0, 0..1);
        }
    }
}

fn group(device: &Device, layouts: &Layouts, entries: &[BindGroupEntry<'_>]) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.section.group"),
        layout: &layouts.section,
        entries,
    })
}

fn style_group(device: &Device, layouts: &Layouts, style: &StyleTable) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.section.style.group"),
        layout: &layouts.section_style,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: style.plane.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: style.styles.buffer().as_entire_binding(),
            },
        ],
    })
}

fn geometry_stamp(scene: &SceneFrame<'_>, plane: Option<PlaneKey>) -> u64 {
    let mut digest = Digest::new();
    match plane {
        Some(key) => {
            for value in key.normal.into_iter().chain(key.right).chain([key.offset]) {
                digest.mix(u64::from(value));
            }
        }
        None => digest.mix(u64::MAX),
    }
    for layer in scene.layers {
        digest.mix(layer.key.digest());
        digest.mix(layer.style_start as u64);
        for style in &scene.styles[layer.style_start..layer.style_start + layer.style_count] {
            digest.flag(style.visible());
        }
    }
    digest.finish()
}

fn style_stamp(styles: &[Style]) -> u64 {
    let mut digest = Digest::new();
    for style in styles {
        for value in style.color() {
            digest.real(value);
        }
        digest.real(style.opacity());
        digest.flag(style.visible());
    }
    digest.finish()
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
