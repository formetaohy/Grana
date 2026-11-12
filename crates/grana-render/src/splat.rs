use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable, cast_slice};
use grana_gpu::{InstanceCommands, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendState, ColorTargetState, ColorWrites,
    CommandEncoder, CompareFunction, ComputePipeline, ComputePipelineDescriptor, Device,
    FragmentState, LoadOp, MultisampleState, Operations, PrimitiveState, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    VertexState,
};

use crate::layout::Layouts;
use crate::renderer::VolumeTargets;
use crate::target::{PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;
use grana_display::srgb_to_linear;

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

const RADIUS_PER_SIGMA: f32 = 1.177_410_1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Splat {
    center: [f32; 4],
    sigma_alpha: [f32; 4],
    color: [f32; 4],
}

impl Splat {
    pub fn point(center: [f32; 3], radius: f32, color: [f32; 3], alpha: f32) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a splat center must be finite"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a splat radius must be a positive length"
        );
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a splat color is an srgb triple"
        );
        assert!(
            (0.0..=1.0).contains(&alpha),
            "a splat alpha lies in the unit interval"
        );
        let sigma = radius / RADIUS_PER_SIGMA;
        let [red, green, blue] = srgb_to_linear(color);
        Self {
            center: [center[0], center[1], center[2], 0.0],
            sigma_alpha: [sigma, sigma, sigma, alpha],
            color: [red, green, blue, 0.0],
        }
    }
}

impl Hash for Splat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.center.map(f32::to_bits).hash(state);
        self.sigma_alpha.map(f32::to_bits).hash(state);
        self.color.map(f32::to_bits).hash(state);
    }
}

#[derive(Clone, Debug)]
pub struct SplatChunk {
    key: u64,
    stamp: u64,
    splats: Vec<Splat>,
}

impl SplatChunk {
    pub fn new(key: u64, splats: Vec<Splat>) -> Self {
        let mut hasher = DefaultHasher::new();
        splats.hash(&mut hasher);
        Self {
            key,
            stamp: hasher.finish(),
            splats,
        }
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn splats(&self) -> &[Splat] {
        &self.splats
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
        self.chunks.iter().all(|chunk| chunk.splats.is_empty())
    }

    pub fn len(self) -> usize {
        self.chunks.iter().map(|chunk| chunk.splats.len()).sum()
    }

    pub fn iter(self) -> impl Iterator<Item = &'a Splat> {
        self.chunks.iter().flat_map(|chunk| chunk.splats.iter())
    }
}

#[derive(Clone, Copy, Debug)]
struct SplatSlot {
    key: u64,
    stamp: u64,
    offset: usize,
    len: usize,
}

pub(crate) struct SplatPasses {
    splats: Table<Splat>,
    order: Table<u32>,
    slots: Vec<SplatSlot>,
    render_group: BindGroup,
    args: InstanceCommands,
    cull_group: BindGroup,
    cull: ComputePipeline,
    slice: RenderPipeline,
    key: RenderPipeline,
    color: RenderPipeline,
    pick: RenderPipeline,
    slice_pick: RenderPipeline,
    count: u32,
}

impl SplatPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let splats = Table::new(device, "grana.splats");
        let order = Table::new(device, "grana.splat.order");
        let args = InstanceCommands::new(device, "grana.splat.args");
        let render_group = render_group(device, layouts, splats.buffer(), order.buffer());
        let cull_group = cull_group(device, layouts, splats.buffer(), order.buffer(), &args);
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
            cache: None,
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
            cache: None,
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
            cache: None,
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
            cache: None,
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
            cache: None,
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
            cache: None,
        });
        Self {
            splats,
            order,
            slots: Vec::new(),
            render_group,
            args,
            cull_group,
            cull,
            slice,
            key,
            color,
            pick,
            slice_pick,
            count: 0,
        }
    }

    pub fn set(&mut self, device: &Device, queue: &Queue, layouts: &Layouts, splats: Splats<'_>) {
        let chunks = splats.chunks();
        let total = splats.len();
        self.count = total as u32;
        if total == 0 {
            self.slots.clear();
            return;
        }
        let mut rebuilt = self.splats.reserve(device, total);
        rebuilt |= self.order.reserve(device, total);
        if rebuilt {
            self.render_group =
                render_group(device, layouts, self.splats.buffer(), self.order.buffer());
            self.cull_group = cull_group(
                device,
                layouts,
                self.splats.buffer(),
                self.order.buffer(),
                &self.args,
            );
        }
        let mut layout = Vec::with_capacity(chunks.len());
        let mut offset = 0usize;
        for chunk in chunks {
            let len = chunk.splats().len();
            let settled = !rebuilt
                && self.slots.iter().any(|slot| {
                    slot.key == chunk.key()
                        && slot.stamp == chunk.stamp()
                        && slot.offset == offset
                        && slot.len == len
                });
            if !settled && len > 0 {
                let start = (offset * size_of::<Splat>()) as u64;
                queue.write_buffer(self.splats.buffer(), start, cast_slice(chunk.splats()));
            }
            layout.push(SplatSlot {
                key: chunk.key(),
                stamp: chunk.stamp(),
                offset,
                len,
            });
            offset += len;
        }
        self.slots = layout;
    }

    pub fn rewind(&self, queue: &Queue) {
        self.args.reset(queue, 6);
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.splat.cull.volume.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.cull);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.cull_group, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
    }

    pub fn draw_slice(
        &self,
        encoder: &mut CommandEncoder,
        targets: VolumeTargets<'_>,
        region: [u32; 4],
        scene: &BindGroup,
    ) {
        if self.count == 0 {
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
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.draw_indirect(self.args.buffer(), 0);
    }

    pub fn draw_oit<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        oit: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.key);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.draw_indirect(self.args.buffer(), 0);
    }

    pub fn draw_color<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        oit: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.color);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.draw_indirect(self.args.buffer(), 0);
    }

    pub fn draw_pick<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pick);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.draw_indirect(self.args.buffer(), 0);
    }

    pub fn draw_pick_slice<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.slice_pick);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.draw_indirect(self.args.buffer(), 0);
    }
}

fn render_group(
    device: &Device,
    layouts: &Layouts,
    splats: &wgpu::Buffer,
    order: &wgpu::Buffer,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.splat.render.group"),
        layout: &layouts.splat_rows,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: splats.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: order.as_entire_binding(),
            },
        ],
    })
}

fn cull_group(
    device: &Device,
    layouts: &Layouts,
    splats: &wgpu::Buffer,
    order: &wgpu::Buffer,
    args: &InstanceCommands,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.splat.cull.group"),
        layout: &layouts.splat_cull,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: splats.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: order.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 5,
                resource: args.buffer().as_entire_binding(),
            },
        ],
    })
}
