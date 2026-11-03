use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_display::Camera;
use grana_geometry::{PackedMesh, SectionCutter, SectionIndex, is_closed};
use grana_gpu::Table;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferUsages,
    ColorTargetState, ColorWrites, Device, FragmentState, IndexFormat, MultisampleState,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPass, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, VertexAttribute,
    VertexBufferLayout, VertexFormat, VertexState, VertexStepMode, util::DeviceExt,
};

use crate::cut;
use crate::frame::SectionData;
use crate::layout::Layouts;
use crate::mesh::{StyleData, style_data};
use crate::schema::{LayerKey, SceneFrame};
use crate::target::{CUT_ID_FORMAT, PICK_FORMAT, SCENE_FORMAT};

const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/slice.wgsl")
);

const UNIT: f32 = 65535.0;
const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
const QUAD_LIMIT: u32 = 1 << 18;
const PARALLEL_FLOOR: usize = 8;
const WORKER_LIMIT: usize = 16;
const INDEX_BYTES_PER_PIXEL: u64 = 64;
const INDEX_BUDGET_FLOOR: u64 = 1 << 20;
const INDEX_BUDGET_CEIL: u64 = 128 << 20;

type Cut = (u32, Vec<[f32; 2]>);

struct Candidate<'a> {
    layer: LayerKey,
    mesh: &'a PackedMesh,
    object: usize,
    style: usize,
    low: [f32; 3],
    high: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FaceVertex {
    uv: [f32; 2],
    style: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FaceCommand {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

const _: () = {
    assert!(size_of::<FaceVertex>() == 12);
    assert!(size_of::<FaceCommand>() == 20);
};

#[derive(Clone, Copy)]
struct FaceRun {
    style: u32,
    first_vertex: u32,
    quads: u32,
    low: [f32; 3],
    high: [f32; 3],
}

pub(crate) struct CutView<'a> {
    pub section: &'a SectionData,
    pub size: [u32; 2],
    pub camera: &'a Camera,
}

pub(crate) struct SlicePass {
    pipeline: RenderPipeline,
    pick_pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
    commands: Buffer,
    styles: Table<StyleData>,
    style_group: BindGroup,
    style_cache: Vec<StyleData>,
    vertex_capacity: u32,
    quad_capacity: u32,
    command_capacity: u32,
    runs: Vec<FaceRun>,
    closed: HashMap<LayerKey, Vec<Option<bool>>>,
    index: SectionCache,
    staged: Vec<FaceVertex>,
    cutter: SectionCutter,
    cut_vertices: Vec<[f32; 2]>,
    span_key: Option<u64>,
    objects: u32,
    draws: u32,
}

impl SlicePass {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.slice.pipeline_layout"),
            bind_group_layouts: &[Some(&layouts.scene), Some(&layouts.styles)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.slice"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.slice"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: size_of::<FaceVertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &[
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: cut::multisample(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    }),
                    Some(ColorTargetState {
                        format: CUT_ID_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let styles = Table::with_capacity(device, "grana.slice.styles", 1);
        let style_group = style_group(device, &layouts.styles, styles.buffer());
        let pick_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.slice.pick.pipeline_layout"),
            bind_group_layouts: &[Some(&layouts.scene)],
            immediate_size: 0,
        });
        let pick_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.slice.pick"),
            layout: Some(&pick_layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: size_of::<FaceVertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &[
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
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
            cache: None,
        });
        Self {
            pipeline,
            pick_pipeline,
            vertices: vertex_buffer(device, "grana.slice.vertices", 4),
            indices: index_buffer(device, "grana.slice.indices", 1),
            commands: command_buffer(device, "grana.slice.commands", 1),
            styles,
            style_group,
            style_cache: Vec::new(),
            vertex_capacity: 4,
            quad_capacity: 1,
            command_capacity: 1,
            runs: Vec::new(),
            closed: HashMap::new(),
            index: SectionCache::new(),
            staged: Vec::new(),
            cutter: SectionCutter::new(),
            cut_vertices: Vec::new(),
            span_key: None,
            objects: 0,
            draws: 0,
        }
    }

    pub fn style_group(&self) -> &BindGroup {
        &self.style_group
    }

    pub fn objects(&self) -> u32 {
        self.objects
    }

    pub fn draws(&self) -> u32 {
        self.draws
    }

    pub fn index_bytes(&self) -> u64 {
        self.index.resident
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
        view: CutView<'_>,
    ) {
        let CutView {
            section,
            size,
            camera,
        } = view;
        if section.uv_scale[0] <= 0.0 || section.uv_scale[1] <= 0.0 {
            self.objects = 0;
            self.draws = 0;
            self.index.clear();
            return;
        }
        self.write_styles(device, queue, layouts, scene);
        let axis = section.mapping[2] as usize;
        let box_min = section.box_min;
        let world = section.world;
        let plane = box_min[axis] + section.plane[0] * world[axis];
        let budget = index_budget(size);
        self.index.fit(budget);
        let key = span_key(scene, axis, plane);
        if self.span_key != Some(key) {
            self.build(scene, axis, plane, budget);
            self.upload(device, queue);
            self.span_key = Some(key);
        }
        self.pose(device, queue, scene, section, camera);
    }

    fn write_styles(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        scene: &SceneFrame<'_>,
    ) {
        let rows = scene
            .styles
            .iter()
            .copied()
            .map(style_data)
            .collect::<Vec<_>>();
        let grown = self.styles.reserve(device, rows.len().max(1));
        if grown {
            self.style_group = style_group(device, &layouts.styles, self.styles.buffer());
        }
        if rows != self.style_cache {
            queue.write_buffer(self.styles.buffer(), 0, bytemuck::cast_slice(&rows));
            self.style_cache = rows;
        }
    }

    fn build(&mut self, scene: &SceneFrame<'_>, axis: usize, plane: f32, budget: u64) {
        self.runs.clear();
        self.staged.clear();
        self.retain(scene);
        let mut candidates = Vec::new();
        for layer in scene.layers {
            let mesh = &layer.mesh;
            for object in 0..mesh.object_count() {
                let group = mesh.groups()[object];
                let low = group.min();
                let high = [0, 1, 2].map(|index| low[index] + UNIT * group.scale()[index]);
                if plane < low[axis] || plane > high[axis] {
                    continue;
                }
                candidates.push(Candidate {
                    layer: layer.key,
                    mesh,
                    object,
                    style: layer.style_start + object,
                    low,
                    high,
                });
            }
        }
        self.keep_solid(&mut candidates);
        self.index.prepare(&candidates, axis, budget);
        let cuts = self.cut(&candidates, axis, plane);
        for (candidate, (quads, vertices)) in candidates.iter().zip(cuts) {
            assert!(
                quads <= QUAD_LIMIT,
                "a cut face spans more than {QUAD_LIMIT} quads"
            );
            if quads == 0 {
                continue;
            }
            let style = candidate.style;
            assert!(style < scene.styles.len(), "a cut face names a scene style");
            let first_vertex = self.staged.len() as u32;
            self.staged
                .extend(vertices.into_iter().map(|uv| FaceVertex {
                    uv,
                    style: style as u32,
                }));
            self.runs.push(FaceRun {
                style: style as u32,
                first_vertex,
                quads,
                low: candidate.low,
                high: candidate.high,
            });
        }
    }

    fn retain(&mut self, scene: &SceneFrame<'_>) {
        self.closed
            .retain(|key, _| scene.layers.iter().any(|layer| layer.key == *key));
        self.index
            .retain(|key| scene.layers.iter().any(|layer| layer.key == *key));
    }

    fn keep_solid(&mut self, candidates: &mut Vec<Candidate<'_>>) {
        let mut solid = Vec::with_capacity(candidates.len());
        let mut missing = Vec::new();
        for (slot, candidate) in candidates.iter().enumerate() {
            let flags = self
                .closed
                .entry(candidate.layer)
                .or_insert_with(|| vec![None; candidate.mesh.object_count()]);
            match flags[candidate.object] {
                Some(flag) => solid.push(flag),
                None => {
                    solid.push(false);
                    missing.push(slot);
                }
            }
        }
        if !missing.is_empty() {
            let found = parallel_over(candidates, &missing, |candidate| {
                is_closed(candidate.mesh, candidate.object)
            });
            for (slot, index) in missing.into_iter().enumerate() {
                let candidate = &candidates[index];
                if let Some(flags) = self.closed.get_mut(&candidate.layer) {
                    flags[candidate.object] = Some(found[slot]);
                }
                solid[index] = found[slot];
            }
        }
        let mut kept = Vec::with_capacity(candidates.len());
        for (candidate, flag) in candidates.drain(..).zip(solid) {
            if flag {
                kept.push(candidate);
            }
        }
        *candidates = kept;
    }

    fn cut(&mut self, candidates: &[Candidate<'_>], axis: usize, plane: f32) -> Vec<Cut> {
        let Self {
            cutter,
            cut_vertices,
            index,
            ..
        } = self;
        if candidates.len() < PARALLEL_FLOOR {
            return candidates
                .iter()
                .map(|candidate| {
                    cut_vertices.clear();
                    let quads = cut_object(cutter, index, candidate, axis, plane, cut_vertices);
                    (quads as u32, std::mem::take(cut_vertices))
                })
                .collect();
        }
        let workers = std::thread::available_parallelism()
            .map_or(1, |value| value.get())
            .min(WORKER_LIMIT)
            .min(candidates.len());
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        let index = &*index;
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let sender = sender.clone();
                let next = &next;
                scope.spawn(move || {
                    let mut cutter = SectionCutter::new();
                    let mut vertices = Vec::new();
                    loop {
                        let slot = next.fetch_add(1, Ordering::Relaxed);
                        if slot >= candidates.len() {
                            break;
                        }
                        vertices.clear();
                        let quads = cut_object(
                            &mut cutter,
                            index,
                            &candidates[slot],
                            axis,
                            plane,
                            &mut vertices,
                        );
                        let quads = quads as u32;
                        let vertices = std::mem::take(&mut vertices);
                        if sender.send((slot, quads, vertices)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
        });
        let mut cuts = vec![(0u32, Vec::new()); candidates.len()];
        let mut filled = 0usize;
        for (index, quads, vertices) in receiver {
            cuts[index] = (quads, vertices);
            filled += 1;
        }
        assert_eq!(filled, candidates.len(), "every cut face is measured");
        cuts
    }

    fn upload(&mut self, device: &Device, queue: &Queue) {
        let vertices = self.staged.len() as u32;
        if vertices > self.vertex_capacity {
            self.vertices = vertex_buffer(device, "grana.slice.vertices", vertices);
            self.vertex_capacity = vertices;
        }
        if !self.staged.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&self.staged));
        }
        let quads = self.runs.iter().map(|run| run.quads).max().unwrap_or(0);
        if quads > self.quad_capacity {
            self.indices = index_buffer(device, "grana.slice.indices", quads);
            self.quad_capacity = quads;
        }
    }

    fn pose(
        &mut self,
        device: &Device,
        queue: &Queue,
        scene: &SceneFrame<'_>,
        section: &SectionData,
        camera: &Camera,
    ) {
        let axis = section.mapping[2] as usize;
        let box_min = section.box_min;
        let world = section.world;
        let plane = box_min[axis] + section.plane[0] * world[axis];
        let eye = camera.eye();
        let forward = camera.forward();
        let mut order = self
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.quads > 0 && scene.styles[run.style as usize].visible())
            .map(|(index, run)| {
                let mut centre = [0.0; 3];
                for (corner, slot) in centre.iter_mut().enumerate() {
                    *slot = 0.5 * (run.low[corner] + run.high[corner]);
                }
                centre[axis] = plane;
                ((Vec3::from(centre) - eye).dot(forward), index)
            })
            .collect::<Vec<_>>();
        order.sort_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));
        let count = order.len() as u32;
        if count > self.command_capacity {
            self.commands = command_buffer(device, "grana.slice.commands", count);
            self.command_capacity = count;
        }
        let rows = order
            .iter()
            .map(|(_, index)| {
                let run = &self.runs[*index];
                FaceCommand {
                    index_count: run.quads * 6,
                    instance_count: 1,
                    first_index: 0,
                    base_vertex: run.first_vertex as i32,
                    first_instance: 0,
                }
            })
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            queue.write_buffer(&self.commands, 0, bytemuck::cast_slice(&rows));
        }
        self.objects = count;
        self.draws = u32::from(count > 0);
    }

    pub fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.draws == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.style_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&self.commands, 0, self.objects);
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.draws == 0 {
            return;
        }
        pass.set_pipeline(&self.pick_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&self.commands, 0, self.objects);
    }
}

fn cut_object(
    cutter: &mut SectionCutter,
    index: &SectionCache,
    candidate: &Candidate<'_>,
    axis: usize,
    plane: f32,
    vertices: &mut Vec<[f32; 2]>,
) -> usize {
    match index.get(candidate.layer, candidate.object, axis) {
        Some(section) => cutter.cut_indexed(
            candidate.mesh,
            candidate.object,
            axis,
            plane,
            section,
            vertices,
        ),
        None => cutter.cut(candidate.mesh, candidate.object, axis, plane, vertices),
    }
}

struct CachedIndex {
    index: SectionIndex,
    used: u64,
    bytes: u64,
}

struct SectionCache {
    entries: HashMap<(LayerKey, usize, usize), CachedIndex>,
    resident: u64,
    budget: u64,
    order: u64,
}

impl SectionCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            resident: 0,
            budget: INDEX_BUDGET_CEIL,
            order: 0,
        }
    }

    fn prepare(&mut self, candidates: &[Candidate<'_>], axis: usize, budget: u64) {
        self.budget = budget;
        self.order += 1;
        let order = self.order;
        let mut missing = Vec::new();
        for (slot, candidate) in candidates.iter().enumerate() {
            let key = (candidate.layer, candidate.object, axis);
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.used = order;
            } else {
                missing.push(slot);
            }
        }
        if !missing.is_empty() {
            let built = parallel_over(candidates, &missing, |candidate| {
                SectionIndex::new(candidate.mesh, candidate.object, axis)
            });
            for (slot, index) in missing.into_iter().zip(built) {
                let candidate = &candidates[slot];
                let key = (candidate.layer, candidate.object, axis);
                let bytes = index.bytes();
                if let Some(previous) = self.entries.insert(
                    key,
                    CachedIndex {
                        index,
                        used: order,
                        bytes,
                    },
                ) {
                    self.resident = self.resident.saturating_sub(previous.bytes);
                }
                self.resident += bytes;
            }
        }
        self.evict(order);
    }

    fn get(&self, layer: LayerKey, object: usize, axis: usize) -> Option<&SectionIndex> {
        self.entries
            .get(&(layer, object, axis))
            .map(|entry| &entry.index)
    }

    fn evict(&mut self, order: u64) {
        while self.resident > self.budget {
            let victim = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.used < order)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key);
            let Some(key) = victim else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.resident -= entry.bytes;
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.resident = 0;
    }

    fn fit(&mut self, budget: u64) {
        self.budget = budget;
        while self.resident > self.budget {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key);
            let Some(key) = victim else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.resident -= entry.bytes;
            }
        }
    }

    fn retain(&mut self, keep: impl Fn(&LayerKey) -> bool) {
        let mut resident = 0u64;
        self.entries.retain(|key, entry| {
            let kept = keep(&key.0);
            if kept {
                resident += entry.bytes;
            }
            kept
        });
        self.resident = resident;
    }
}

fn parallel_over<U: Send>(
    candidates: &[Candidate<'_>],
    slots: &[usize],
    work: impl Fn(&Candidate<'_>) -> U + Sync,
) -> Vec<U> {
    let total = slots.len();
    let workers = std::thread::available_parallelism()
        .map_or(1, |value| value.get())
        .min(WORKER_LIMIT)
        .min(total);
    if workers <= 1 {
        return slots.iter().map(|slot| work(&candidates[*slot])).collect();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            let work = &work;
            scope.spawn(move || {
                loop {
                    let position = next.fetch_add(1, Ordering::Relaxed);
                    if position >= total {
                        break;
                    }
                    let outcome = work(&candidates[slots[position]]);
                    if sender.send((position, outcome)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
    });
    let mut results = (0..total).map(|_| None).collect::<Vec<Option<U>>>();
    for (position, outcome) in receiver {
        results[position] = Some(outcome);
    }
    results
        .into_iter()
        .map(|outcome| outcome.expect("every cut plane task completes"))
        .collect()
}

fn index_budget(size: [u32; 2]) -> u64 {
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    (pixels * INDEX_BYTES_PER_PIXEL).clamp(INDEX_BUDGET_FLOOR, INDEX_BUDGET_CEIL)
}

fn span_key(scene: &SceneFrame<'_>, axis: usize, plane: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    axis.hash(&mut hasher);
    plane.to_bits().hash(&mut hasher);
    for layer in scene.layers {
        layer.key.hash(&mut hasher);
        layer.style_start.hash(&mut hasher);
        layer.mesh.vertex_count().hash(&mut hasher);
    }
    hasher.finish()
}

fn style_group(device: &Device, layout: &BindGroupLayout, buffer: &Buffer) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.slice.style.group"),
        layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn vertex_buffer(device: &Device, label: &'static str, vertices: u32) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: u64::from(vertices.max(1)) * size_of::<FaceVertex>() as u64,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn index_buffer(device: &Device, label: &'static str, quads: u32) -> Buffer {
    let pattern = (0..quads)
        .flat_map(|quad| QUAD_INDICES.map(|index| index + 4 * quad))
        .collect::<Vec<_>>();
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(&pattern),
        usage: BufferUsages::INDEX,
    })
}

fn command_buffer(device: &Device, label: &'static str, commands: u32) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: u64::from(commands.max(1)) * size_of::<FaceCommand>() as u64,
        usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
