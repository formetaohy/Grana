use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytemuck::{Pod, Zeroable, bytes_of, cast_slice};
use grana_gpu::Pipelines;
use grana_gpu::{InstanceCommands, storage_buffer};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendState, Buffer, BufferDescriptor,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, ComputePipeline,
    ComputePipelineDescriptor, Device, FragmentState, LoadOp, MultisampleState, Operations,
    PrimitiveState, Queue, RenderPass, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, StoreOp, VertexState,
};

use crate::layout::Layouts;
use crate::renderer::VolumeTargets;
use crate::target::{PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;

const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/splat_cull.wgsl")
);
const SPLAT_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/oit.wgsl"),
    "\n",
    include_str!("shader/splat.wgsl")
);

const CULL_WORKGROUP: u32 = 64;
const VERTEX_COUNT: u32 = 6;

pub const SPLAT_LEVELS: u8 = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SplatGeometry {
    center: [f32; 4],
    sigma: [f32; 4],
}

impl SplatGeometry {
    pub fn gaussian(center: [f32; 3], sigma: [f32; 3]) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a splat center must be finite"
        );
        assert!(
            sigma.iter().all(|value| value.is_finite() && *value > 0.0),
            "a splat sigma is a positive length on every axis"
        );
        Self {
            center: [center[0], center[1], center[2], 0.0],
            sigma: [sigma[0], sigma[1], sigma[2], 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SplatStyle {
    color: [f32; 4],
}

impl SplatStyle {
    pub fn new(radiance: [f32; 3], alpha: f32) -> Self {
        assert!(
            radiance.iter().all(|value| (0.0..=1.0).contains(value)),
            "a splat radiance is a linear triple"
        );
        assert!(
            (0.0..=1.0).contains(&alpha),
            "a splat alpha lies in the unit interval"
        );
        Self {
            color: [radiance[0], radiance[1], radiance[2], alpha],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Splat {
    geometry: SplatGeometry,
    style: SplatStyle,
}

impl Splat {
    pub fn gaussian(center: [f32; 3], sigma: [f32; 3], radiance: [f32; 3], alpha: f32) -> Self {
        assert!(
            radiance.iter().all(|value| (0.0..=1.0).contains(value)),
            "a splat radiance is a linear triple"
        );
        Self {
            geometry: SplatGeometry::gaussian(center, sigma),
            style: SplatStyle::new(radiance, alpha),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SplatLevel {
    geometry_stamp: u64,
    style_stamp: u64,
    geometry: Arc<Vec<SplatGeometry>>,
    style: Arc<Vec<SplatStyle>>,
}

impl SplatLevel {
    pub fn new(
        geometry_stamp: u64,
        style_stamp: u64,
        geometry: Arc<Vec<SplatGeometry>>,
        style: Arc<Vec<SplatStyle>>,
    ) -> Self {
        assert_eq!(
            geometry.len(),
            style.len(),
            "a splat level styles every one of its instances"
        );
        Self {
            geometry_stamp,
            style_stamp,
            geometry,
            style,
        }
    }

    pub fn of(stamp: u64, splats: Vec<Splat>) -> Self {
        let mut geometry = Vec::with_capacity(splats.len());
        let mut style = Vec::with_capacity(splats.len());
        for splat in splats {
            geometry.push(splat.geometry);
            style.push(splat.style);
        }
        Self::new(stamp, stamp, Arc::new(geometry), Arc::new(style))
    }

    pub fn geometry_stamp(&self) -> u64 {
        self.geometry_stamp
    }

    pub fn style_stamp(&self) -> u64 {
        self.style_stamp
    }

    pub fn geometry(&self) -> &[SplatGeometry] {
        &self.geometry
    }

    pub fn style(&self) -> &[SplatStyle] {
        &self.style
    }

    pub fn len(&self) -> usize {
        self.geometry.len()
    }

    pub fn is_empty(&self) -> bool {
        self.geometry.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct SplatChunk {
    key: u64,
    level: u8,
    active: bool,
    pickable: bool,
    data: SplatLevel,
}

impl SplatChunk {
    pub fn new(key: u64, level: u8, active: bool, data: SplatLevel) -> Self {
        assert!(
            level < SPLAT_LEVELS,
            "a splat chunk names one of its resident levels"
        );
        Self {
            key,
            level,
            active,
            pickable: true,
            data,
        }
    }

    pub fn pickable(mut self, pickable: bool) -> Self {
        self.pickable = pickable;
        self
    }

    pub fn is_pickable(&self) -> bool {
        self.pickable
    }

    pub fn single(key: u64, stamp: u64, splats: Vec<Splat>) -> Self {
        Self::new(key, 0, true, SplatLevel::of(stamp, splats))
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn data(&self) -> &SplatLevel {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Splats<'a> {
    chunks: &'a [SplatChunk],
}

impl<'a> Splats<'a> {
    pub fn empty() -> Self {
        Self { chunks: &[] }
    }

    pub fn new(chunks: &'a [SplatChunk]) -> Self {
        Self { chunks }
    }

    pub fn chunks(self) -> &'a [SplatChunk] {
        self.chunks
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn len(self) -> usize {
        self.chunks
            .iter()
            .filter(|chunk| chunk.active)
            .map(SplatChunk::len)
            .sum()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Base {
    slot: [u32; 4],
}

struct SplatSlot {
    base: u32,
    len: usize,
    geometry_stamp: u64,
    style_stamp: u64,
    geometry: Buffer,
    style: Buffer,
    args: InstanceCommands,
    rows: BindGroup,
    cull: BindGroup,
    pick: BindGroup,
}

impl SplatSlot {
    fn new(
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        base: u32,
        data: &SplatLevel,
        scratch: &mut Vec<SplatGeometry>,
    ) -> Self {
        let len = data.len();
        assert!(len > 0, "a splat slot holds at least one instance");
        let geometry = storage_buffer::<SplatGeometry>(device, "grana.splat.geometry", len);
        let style = storage_buffer::<SplatStyle>(device, "grana.splat.style", len);
        let order = storage_buffer::<u32>(device, "grana.splat.order", len);
        let args = InstanceCommands::new(device, "grana.splat.args");
        let base_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("grana.splat.base"),
            size: size_of::<Base>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &base_buffer,
            0,
            bytes_of(&Base {
                slot: [base, 0, 0, 0],
            }),
        );
        let rows = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.splat.rows.group"),
            layout: &layouts.splat_rows,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: geometry.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: order.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: style.as_entire_binding(),
                },
            ],
        });
        let cull = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.splat.cull.group"),
            layout: &layouts.splat_cull,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: geometry.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: order.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: style.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: args.buffer().as_entire_binding(),
                },
            ],
        });
        let pick = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.splat.pick.slot.group"),
            layout: &layouts.splat_pick_slot,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: base_buffer.as_entire_binding(),
            }],
        });
        let mut slot = Self {
            base,
            len,
            geometry_stamp: u64::MAX,
            style_stamp: u64::MAX,
            geometry,
            style,
            args,
            rows,
            cull,
            pick,
        };
        slot.write(queue, data, scratch);
        slot
    }

    fn write(&mut self, queue: &Queue, data: &SplatLevel, scratch: &mut Vec<SplatGeometry>) {
        if self.geometry_stamp != data.geometry_stamp() {
            scratch.clear();
            scratch.extend_from_slice(data.geometry());
            for cell in scratch.iter_mut() {
                cell.center[3] = self.base as f32;
            }
            queue.write_buffer(&self.geometry, 0, cast_slice(scratch));
            self.geometry_stamp = data.geometry_stamp();
        }
        if self.style_stamp != data.style_stamp() {
            queue.write_buffer(&self.style, 0, cast_slice(data.style()));
            self.style_stamp = data.style_stamp();
        }
    }
}

pub(crate) struct SplatPasses {
    slots: HashMap<(u64, u8), SplatSlot>,
    active: Vec<(u64, u8)>,
    pickable: Vec<(u64, u8)>,
    ranges: Vec<(u32, u32, u64, u8)>,
    next_base: u32,
    count: u32,
    scratch: Vec<SplatGeometry>,
    cull: ComputePipeline,
    slice: RenderPipeline,
    key: RenderPipeline,
    color: RenderPipeline,
    pick: RenderPipeline,
    slice_pick: RenderPipeline,
}

impl SplatPasses {
    pub fn new(device: &Device, layouts: &Layouts, pipelines: &Pipelines) -> Self {
        let cull_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.splat.cull"),
            source: ShaderSource::Wgsl(CULL_SHADER.into()),
        });
        let cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.splat.cull.volume"),
            layout: Some(&layouts.splat_cull_pipeline),
            module: &cull_module,
            entry_point: Some("splat_cull_volume"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.splat"),
            source: ShaderSource::Wgsl(SPLAT_SHADER.into()),
        });
        let slice = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.splat.slice"),
            layout: Some(&layouts.splat_slice),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(false, CompareFunction::LessEqual)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_slice"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let key = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.splat.oit.key"),
            layout: Some(&layouts.splat_oit),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(false, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_oit_key"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let color = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.splat.oit.color"),
            layout: Some(&layouts.splat_oit),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(false, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_oit_color"),
                compilation_options: Default::default(),
                targets: &crate::oit::residual_targets(),
            }),
            multiview_mask: None,
            cache: pipelines.cache(),
        });
        let pick = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.splat.pick"),
            layout: Some(&layouts.splat_pick),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
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
        let slice_pick = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.splat.pick.slice"),
            layout: Some(&layouts.splat_pick),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(true, CompareFunction::LessEqual)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs_pick_slice"),
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
            slots: HashMap::new(),
            active: Vec::new(),
            pickable: Vec::new(),
            ranges: Vec::new(),
            next_base: 0,
            count: 0,
            scratch: Vec::new(),
            cull,
            slice,
            key,
            color,
            pick,
            slice_pick,
        }
    }

    pub fn set(&mut self, device: &Device, queue: &Queue, layouts: &Layouts, splats: Splats<'_>) {
        let mut seen = HashSet::new();
        self.active.clear();
        self.pickable.clear();
        let mut count = 0usize;
        for chunk in splats.chunks() {
            if chunk.is_empty() {
                continue;
            }
            let entry = (chunk.key(), chunk.level());
            seen.insert(entry);
            count = count.max(chunk.len());
            let resize = self
                .slots
                .get(&entry)
                .is_none_or(|slot| slot.len != chunk.len());
            if resize {
                let base = self.base_for(entry, chunk.len());
                let slot = SplatSlot::new(
                    device,
                    queue,
                    layouts,
                    base,
                    chunk.data(),
                    &mut self.scratch,
                );
                self.slots.insert(entry, slot);
            } else if let Some(slot) = self.slots.get_mut(&entry) {
                slot.write(queue, chunk.data(), &mut self.scratch);
            }
            if chunk.active() {
                self.active.push(entry);
                if chunk.is_pickable() {
                    self.pickable.push(entry);
                }
            }
        }
        self.slots.retain(|entry, _| seen.contains(entry));
        self.count = count as u32;
        self.ranges.clear();
        for (entry, slot) in &self.slots {
            self.ranges
                .push((slot.base, slot.base + slot.len as u32, entry.0, entry.1));
        }
        self.ranges.sort_unstable();
    }

    fn base_for(&mut self, entry: (u64, u8), len: usize) -> u32 {
        let len = len as u32;
        let held = self.slots.get(&entry).map(|slot| slot.base);
        let free = held.filter(|base| {
            let end = base.saturating_add(len);
            end > *base
                && !self.slots.iter().any(|(key, slot)| {
                    *key != entry
                        && slot.base < end
                        && *base < slot.base.saturating_add(slot.len as u32)
                })
        });
        free.unwrap_or_else(|| {
            let base = self.next_base;
            self.next_base = base
                .checked_add(len)
                .expect("the splat instance space is exhausted");
            base
        })
    }

    pub fn resolve(&self, index: u32) -> Option<(u64, u8, u32)> {
        let (start, _, key, level) = self
            .ranges
            .iter()
            .find(|(start, end, ..)| index >= *start && index < *end)?;
        Some((*key, *level, index - start))
    }

    pub fn rewind(&self, queue: &Queue) {
        for entry in &self.active {
            let slot = &self.slots[entry];
            slot.args.reset(queue, VERTEX_COUNT);
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        if self.active.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.splat.cull.volume.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.cull);
        pass.set_bind_group(0, scene, &[]);
        for entry in &self.active {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.cull, &[]);
            pass.dispatch_workgroups((slot.len as u32).div_ceil(CULL_WORKGROUP), 1, 1);
        }
    }

    pub fn draw_slice(
        &self,
        encoder: &mut CommandEncoder,
        targets: VolumeTargets<'_>,
        region: [u32; 4],
        scene: &BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.splat.slice.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: targets.color,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: targets.depth,
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
        pass.set_pipeline(&self.slice);
        pass.set_bind_group(0, scene, &[]);
        for entry in &self.active {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.rows, &[]);
            pass.draw_indirect(slot.args.buffer(), 0);
        }
    }

    pub fn draw_oit<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        oit: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.key);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(4, oit, &[]);
        for entry in &self.active {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.rows, &[]);
            pass.draw_indirect(slot.args.buffer(), 0);
        }
    }

    pub fn draw_color<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        oit: &'a BindGroup,
    ) {
        if self.active.is_empty() {
            return;
        }
        pass.set_pipeline(&self.color);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(4, oit, &[]);
        for entry in &self.active {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.rows, &[]);
            pass.draw_indirect(slot.args.buffer(), 0);
        }
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.pickable.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pick);
        pass.set_bind_group(0, scene, &[]);
        for entry in &self.pickable {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.rows, &[]);
            pass.set_bind_group(2, &slot.pick, &[]);
            pass.draw_indirect(slot.args.buffer(), 0);
        }
    }

    pub fn draw_pick_slice<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.pickable.is_empty() {
            return;
        }
        pass.set_pipeline(&self.slice_pick);
        pass.set_bind_group(0, scene, &[]);
        for entry in &self.pickable {
            let slot = &self.slots[entry];
            pass.set_bind_group(1, &slot.rows, &[]);
            pass.set_bind_group(2, &slot.pick, &[]);
            pass.draw_indirect(slot.args.buffer(), 0);
        }
    }
}
