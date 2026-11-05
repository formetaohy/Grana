use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable};
use grana_gpu::{InstanceCommands, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendState, ColorTargetState, ColorWrites,
    CommandEncoder, CompareFunction, ComputePipeline, ComputePipelineDescriptor, Device,
    FragmentState, LoadOp, MultisampleState, Operations, PrimitiveState, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexState,
};

use crate::cut;
use crate::layout::Layouts;
use crate::target::{PICK_FORMAT, SCENE_FORMAT};
use crate::volume::depth_state;
use grana_display::srgb_to_linear;

const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_cull.wgsl")
);
const VOLUME_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/oit.wgsl"),
    "\n",
    include_str!("shader/marker.wgsl")
);
const SECTION_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_section.wgsl")
);
const SHADOW_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_shadow.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Marker {
    center_radius: [f32; 4],
    color_alpha: [f32; 4],
}

impl Marker {
    pub fn new(center: [f32; 3], radius: f32, color: [f32; 3], alpha: f32) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a marker center must be finite"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a marker radius must be a positive length"
        );
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a marker color is an srgb triple"
        );
        assert!(
            (0.0..=1.0).contains(&alpha),
            "a marker alpha lies in the unit interval"
        );
        let [red, green, blue] = srgb_to_linear(color);
        Self {
            center_radius: [center[0], center[1], center[2], radius],
            color_alpha: [red, green, blue, alpha],
        }
    }

    pub(crate) fn center(self) -> [f32; 3] {
        [
            self.center_radius[0],
            self.center_radius[1],
            self.center_radius[2],
        ]
    }

    pub(crate) fn radius(self) -> f32 {
        self.center_radius[3]
    }

    pub(crate) fn alpha(self) -> f32 {
        self.color_alpha[3]
    }

    pub(crate) fn transparent(self) -> bool {
        self.alpha() < 1.0
    }
}

impl Hash for Marker {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.center_radius.map(f32::to_bits).hash(state);
        self.color_alpha.map(f32::to_bits).hash(state);
    }
}

pub(crate) struct MarkerPasses {
    markers: Table<Marker>,
    opaque_order: Table<u32>,
    layer_order: Table<u32>,
    behind_order: Table<u32>,
    front_order: Table<u32>,
    opaque_group: BindGroup,
    layer_group: BindGroup,
    behind_group: BindGroup,
    front_group: BindGroup,
    shadow_group: BindGroup,
    cull_group: BindGroup,
    opaque_args: InstanceCommands,
    layer_args: InstanceCommands,
    behind_args: InstanceCommands,
    front_args: InstanceCommands,
    volume_cull: ComputePipeline,
    behind_cull: ComputePipeline,
    front_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    oit_pipeline: RenderPipeline,
    tail_pipeline: RenderPipeline,
    section_pipeline: RenderPipeline,
    section_pick_pipeline: RenderPipeline,
    volume_pick_pipeline: RenderPipeline,
    shadow_pipeline: RenderPipeline,
    count: u32,
}

impl MarkerPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let markers = Table::new(device, "grana.markers");
        let opaque_order = Table::new(device, "grana.marker.opaque.order");
        let layer_order = Table::new(device, "grana.marker.layer.order");
        let behind_order = Table::new(device, "grana.marker.behind.order");
        let front_order = Table::new(device, "grana.marker.front.order");
        let opaque_args = InstanceCommands::new(device, "grana.marker.opaque.args");
        let layer_args = InstanceCommands::new(device, "grana.marker.layer.args");
        let behind_args = InstanceCommands::new(device, "grana.marker.behind.args");
        let front_args = InstanceCommands::new(device, "grana.marker.front.args");
        let groups = groups(
            device,
            layouts,
            markers.buffer(),
            [
                opaque_order.buffer(),
                layer_order.buffer(),
                behind_order.buffer(),
                front_order.buffer(),
            ],
            [
                opaque_args.buffer(),
                layer_args.buffer(),
                behind_args.buffer(),
                front_args.buffer(),
            ],
        );
        let cull_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker.cull"),
            source: ShaderSource::Wgsl(CULL_SHADER.into()),
        });
        let volume_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.marker.cull.volume"),
            layout: Some(&layouts.marker_cull_pipeline),
            module: &cull_module,
            entry_point: Some("marker_cull_volume"),
            compilation_options: Default::default(),
            cache: None,
        });
        let behind_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.marker.cull.section.behind"),
            layout: Some(&layouts.marker_cull_pipeline),
            module: &cull_module,
            entry_point: Some("marker_cull_section_behind"),
            compilation_options: Default::default(),
            cache: None,
        });
        let front_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.marker.cull.section.front"),
            layout: Some(&layouts.marker_cull_pipeline),
            module: &cull_module,
            entry_point: Some("marker_cull_section_front"),
            compilation_options: Default::default(),
            cache: None,
        });
        let volume_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker"),
            source: ShaderSource::Wgsl(VOLUME_SHADER.into()),
        });
        let section_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker.section"),
            source: ShaderSource::Wgsl(SECTION_SHADER.into()),
        });
        let shadow_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker.shadow"),
            source: ShaderSource::Wgsl(SHADOW_SHADER.into()),
        });
        let volume_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker"),
            layout: Some(&layouts.marker_volume),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: crate::target::scene_multisample(),
            fragment: Some(FragmentState {
                module: &volume_module,
                entry_point: Some("fs_main"),
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
        let oit_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.oit"),
            layout: Some(&layouts.marker_oit),
            vertex: VertexState {
                module: &volume_module,
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
                module: &volume_module,
                entry_point: Some("fs_oit"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let tail_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.tail"),
            layout: Some(&layouts.marker_tail),
            vertex: VertexState {
                module: &volume_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_module,
                entry_point: Some("fs_tail"),
                compilation_options: Default::default(),
                targets: &crate::oit::tail_targets(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let section_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.section"),
            layout: Some(&layouts.marker_section),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: cut::multisample(),
            fragment: Some(FragmentState {
                module: &section_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    }),
                    Some(ColorTargetState {
                        format: crate::target::CUT_ID_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::empty(),
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.shadow"),
            layout: Some(&layouts.marker_volume),
            vertex: VertexState {
                module: &shadow_module,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, CompareFunction::Less)),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shadow_module,
                entry_point: Some("fs_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let volume_pick_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.pick.volume"),
            layout: Some(&layouts.marker_volume),
            vertex: VertexState {
                module: &volume_module,
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
        let section_pick_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.pick.section"),
            layout: Some(&layouts.marker_section),
            vertex: VertexState {
                module: &section_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_module,
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
            markers,
            opaque_order,
            layer_order,
            behind_order,
            front_order,
            opaque_group: groups.opaque_render,
            layer_group: groups.layer_render,
            behind_group: groups.behind_render,
            front_group: groups.front_render,
            shadow_group: groups.shadow_render,
            cull_group: groups.cull,
            opaque_args,
            layer_args,
            behind_args,
            front_args,
            volume_cull,
            behind_cull,
            front_cull,
            volume_pipeline,
            oit_pipeline,
            tail_pipeline,
            section_pipeline,
            section_pick_pipeline,
            volume_pick_pipeline,
            shadow_pipeline,
            count: 0,
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        layouts: &Layouts,
        markers: &[Marker],
    ) {
        if markers.is_empty() {
            self.count = 0;
            return;
        }
        self.count = markers.len() as u32;
        let mut stale = self.markers.write(device, queue, markers);
        stale |= self.opaque_order.reserve(device, markers.len());
        stale |= self.layer_order.reserve(device, markers.len());
        stale |= self.behind_order.reserve(device, markers.len());
        stale |= self.front_order.reserve(device, markers.len());
        if stale {
            let groups = groups(
                device,
                layouts,
                self.markers.buffer(),
                [
                    self.opaque_order.buffer(),
                    self.layer_order.buffer(),
                    self.behind_order.buffer(),
                    self.front_order.buffer(),
                ],
                [
                    self.opaque_args.buffer(),
                    self.layer_args.buffer(),
                    self.behind_args.buffer(),
                    self.front_args.buffer(),
                ],
            );
            self.opaque_group = groups.opaque_render;
            self.layer_group = groups.layer_render;
            self.behind_group = groups.behind_render;
            self.front_group = groups.front_render;
            self.shadow_group = groups.shadow_render;
            self.cull_group = groups.cull;
        }
        self.opaque_args.reset(queue, 6);
        self.layer_args.reset(queue, 6);
        self.behind_args.reset(queue, 6);
        self.front_args.reset(queue, 6);
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull(
            encoder,
            scene,
            &self.volume_cull,
            &self.cull_group,
            "grana.marker.cull.volume.pass",
        );
    }

    pub fn cull_section(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull(
            encoder,
            scene,
            &self.behind_cull,
            &self.cull_group,
            "grana.marker.cull.section.behind.pass",
        );
        self.cull(
            encoder,
            scene,
            &self.front_cull,
            &self.cull_group,
            "grana.marker.cull.section.front.pass",
        );
    }

    fn cull(
        &self,
        encoder: &mut CommandEncoder,
        scene: &BindGroup,
        pipeline: &ComputePipeline,
        group: &BindGroup,
        label: &str,
    ) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
    }

    pub fn draw_volume(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        depth: &TextureView,
        region: [u32; 4],
        scene: &BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.pass"),
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
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.draw_indirect(self.opaque_args.buffer(), 0);
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
        pass.set_pipeline(&self.oit_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.draw_indirect(self.layer_args.buffer(), 0);
    }

    pub fn draw_tail<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        oit: &'a BindGroup,
        depth: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.tail_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(5, depth, &[]);
        pass.draw_indirect(self.layer_args.buffer(), 0);
    }

    pub fn draw_shadow(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        scene: &BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.shadow.pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.draw(0..6, 0..self.count);
    }

    pub fn draw_section_behind<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section(pass, scene, &self.behind_group, &self.behind_args);
    }

    pub fn draw_section_front<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section(pass, scene, &self.front_group, &self.front_args);
    }

    pub fn draw_pick_volume<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.volume_pick_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.draw_indirect(self.opaque_args.buffer(), 0);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.draw_indirect(self.layer_args.buffer(), 0);
    }

    pub fn draw_pick_section_behind<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section_pick(pass, scene, &self.behind_group, &self.behind_args);
    }

    pub fn draw_pick_section_front<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section_pick(pass, scene, &self.front_group, &self.front_args);
    }

    fn draw_section_pick<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        group: &'a BindGroup,
        args: &'a InstanceCommands,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.section_pick_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw_indirect(args.buffer(), 0);
    }

    fn draw_section<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        group: &'a BindGroup,
        args: &'a InstanceCommands,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.section_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw_indirect(args.buffer(), 0);
    }
}

struct Groups {
    opaque_render: BindGroup,
    layer_render: BindGroup,
    behind_render: BindGroup,
    front_render: BindGroup,
    shadow_render: BindGroup,
    cull: BindGroup,
}

fn groups(
    device: &Device,
    layouts: &Layouts,
    markers: &wgpu::Buffer,
    orders: [&wgpu::Buffer; 4],
    args: [&wgpu::Buffer; 4],
) -> Groups {
    let [opaque_order, layer_order, behind_order, front_order] = orders;
    let [opaque_args, layer_args, behind_args, front_args] = args;
    let render = |label: &str, order: &wgpu::Buffer| {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.marker_render,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: markers.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: order.as_entire_binding(),
                },
            ],
        })
    };
    let cull = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.marker.cull.group"),
        layout: &layouts.marker_cull,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: markers.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: opaque_order.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: layer_order.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: behind_order.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: front_order.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 5,
                resource: opaque_args.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 6,
                resource: layer_args.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 7,
                resource: behind_args.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 8,
                resource: front_args.as_entire_binding(),
            },
        ],
    });
    Groups {
        opaque_render: render("grana.marker.opaque.render.group", opaque_order),
        layer_render: render("grana.marker.layer.render.group", layer_order),
        behind_render: render("grana.marker.behind.render.group", behind_order),
        front_render: render("grana.marker.front.render.group", front_order),
        shadow_render: render("grana.marker.shadow.render.group", opaque_order),
        cull,
    }
}
