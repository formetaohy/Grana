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

use crate::layout::Layouts;
use crate::renderer::{Lighting, VolumeTargets};
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
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/brdf.wgsl"),
    "\n",
    include_str!("shader/oit.wgsl"),
    "\n",
    include_str!("shader/marker.wgsl")
);
const SHADOW_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_shadow.wgsl")
);
const AO_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_ao.wgsl")
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
    opaque_group: BindGroup,
    layer_group: BindGroup,
    shadow_group: BindGroup,
    args: MarkerArgs,
    cull_group: BindGroup,
    volume_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    oit_pipeline: RenderPipeline,
    tail_pipeline: RenderPipeline,
    color_pipeline: RenderPipeline,
    volume_pick_pipeline: RenderPipeline,
    shadow_pipeline: RenderPipeline,
    ao_pipeline: RenderPipeline,
    count: u32,
}

struct MarkerArgs {
    opaque: InstanceCommands,
    layer: InstanceCommands,
}

impl MarkerPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let markers = Table::new(device, "grana.markers");
        let opaque_order = Table::new(device, "grana.marker.opaque.order");
        let layer_order = Table::new(device, "grana.marker.layer.order");
        let args = MarkerArgs {
            opaque: InstanceCommands::new(device, "grana.marker.opaque.args"),
            layer: InstanceCommands::new(device, "grana.marker.layer.args"),
        };
        let groups = groups(
            device,
            layouts,
            markers.buffer(),
            opaque_order.buffer(),
            layer_order.buffer(),
        );
        let cull_group = cull_group(
            device,
            layouts,
            markers.buffer(),
            opaque_order.buffer(),
            layer_order.buffer(),
            &args,
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
        let volume_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker"),
            source: ShaderSource::Wgsl(VOLUME_SHADER.into()),
        });
        let shadow_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker.shadow"),
            source: ShaderSource::Wgsl(SHADOW_SHADER.into()),
        });
        let ao_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker.ao"),
            source: ShaderSource::Wgsl(AO_SHADER.into()),
        });
        let ao_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.ao"),
            layout: Some(&layouts.ao_marker),
            vertex: VertexState {
                module: &ao_module,
                entry_point: Some("vs_ao"),
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
                module: &ao_module,
                entry_point: Some("fs_ao"),
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
            multisample: MultisampleState::default(),
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
                entry_point: Some("fs_oit_tail"),
                compilation_options: Default::default(),
                targets: &crate::oit::tail_targets(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let color_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.oit.color"),
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
        let shadow_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker.shadow"),
            layout: Some(&layouts.marker_shadow),
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
            layout: Some(&layouts.marker_pick),
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
        Self {
            markers,
            opaque_order,
            layer_order,
            opaque_group: groups.opaque_render,
            layer_group: groups.layer_render,
            shadow_group: groups.shadow_render,
            args,
            cull_group,
            volume_cull,
            volume_pipeline,
            oit_pipeline,
            tail_pipeline,
            color_pipeline,
            volume_pick_pipeline,
            shadow_pipeline,
            ao_pipeline,
            count: 0,
        }
    }

    pub fn set(&mut self, device: &Device, queue: &Queue, layouts: &Layouts, markers: &[Marker]) {
        if markers.is_empty() {
            self.count = 0;
            return;
        }
        self.count = markers.len() as u32;
        let mut stale = self.markers.write(device, queue, markers);
        stale |= self.opaque_order.reserve(device, markers.len());
        stale |= self.layer_order.reserve(device, markers.len());
        if stale {
            let groups = groups(
                device,
                layouts,
                self.markers.buffer(),
                self.opaque_order.buffer(),
                self.layer_order.buffer(),
            );
            self.opaque_group = groups.opaque_render;
            self.layer_group = groups.layer_render;
            self.shadow_group = groups.shadow_render;
            self.cull_group = cull_group(
                device,
                layouts,
                self.markers.buffer(),
                self.opaque_order.buffer(),
                self.layer_order.buffer(),
                &self.args,
            );
        }
    }

    pub fn rewind(&self, queue: &Queue) {
        self.args.opaque.reset(queue, 6);
        self.args.layer.reset(queue, 6);
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.marker.cull.volume.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.volume_cull);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.cull_group, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
    }

    pub fn draw_gbuffer(
        &self,
        encoder: &mut CommandEncoder,
        normal: &TextureView,
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
            label: Some("grana.marker.ao.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: normal,
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
        pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, width, height);
        pass.set_pipeline(&self.ao_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.draw_indirect(self.args.opaque.buffer(), 0);
    }

    pub fn draw_volume(
        &self,
        encoder: &mut CommandEncoder,
        targets: VolumeTargets<'_>,
        region: [u32; 4],
        scene: &BindGroup,
        lighting: Lighting<'_>,
    ) {
        self.draw_markers(encoder, targets, region, scene, lighting, false);
    }
    pub fn draw_slice(
        &self,
        encoder: &mut CommandEncoder,
        targets: VolumeTargets<'_>,
        region: [u32; 4],
        scene: &BindGroup,
        lighting: Lighting<'_>,
    ) {
        self.draw_markers(encoder, targets, region, scene, lighting, true);
    }

    fn draw_markers(
        &self,
        encoder: &mut CommandEncoder,
        targets: VolumeTargets<'_>,
        region: [u32; 4],
        scene: &BindGroup,
        lighting: Lighting<'_>,
        layered: bool,
    ) {
        if self.count == 0 {
            return;
        }
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.pass"),
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
        pass.set_pipeline(&self.volume_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.set_bind_group(2, lighting.ao, &[]);
        pass.set_bind_group(3, lighting.shadow, &[]);
        pass.draw_indirect(self.args.opaque.buffer(), 0);
        if layered {
            pass.set_bind_group(1, &self.layer_group, &[]);
            pass.draw_indirect(self.args.layer.buffer(), 0);
        }
    }

    pub fn draw_oit<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        ao: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.oit_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.set_bind_group(2, ao, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.draw_indirect(self.args.layer.buffer(), 0);
    }

    pub fn draw_color<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        coverage: &'a BindGroup,
        ao: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.color_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.set_bind_group(2, ao, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(6, coverage, &[]);
        pass.draw_indirect(self.args.layer.buffer(), 0);
    }

    pub fn draw_tail<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        shadow: &'a BindGroup,
        oit: &'a BindGroup,
        coverage: &'a BindGroup,
        ao: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.tail_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.set_bind_group(2, ao, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.set_bind_group(4, oit, &[]);
        pass.set_bind_group(6, coverage, &[]);
        pass.draw_indirect(self.args.layer.buffer(), 0);
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
        pass.set_bind_group(1, &self.shadow_group, &[]);
        pass.draw(0..6, 0..self.count);
    }

    pub fn draw_pick_volume<'a>(
        &'a self,
        pass: &mut RenderPass<'a>,
        scene: &'a BindGroup,
        shadow: &'a BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.volume_pick_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.opaque_group, &[]);
        pass.set_bind_group(3, shadow, &[]);
        pass.draw_indirect(self.args.opaque.buffer(), 0);
        pass.set_bind_group(1, &self.layer_group, &[]);
        pass.draw_indirect(self.args.layer.buffer(), 0);
    }
}

struct Groups {
    opaque_render: BindGroup,
    layer_render: BindGroup,
    shadow_render: BindGroup,
}

fn groups(
    device: &Device,
    layouts: &Layouts,
    markers: &wgpu::Buffer,
    opaque_order: &wgpu::Buffer,
    layer_order: &wgpu::Buffer,
) -> Groups {
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
    Groups {
        opaque_render: render("grana.marker.opaque.render.group", opaque_order),
        layer_render: render("grana.marker.layer.render.group", layer_order),
        shadow_render: render("grana.marker.shadow.render.group", opaque_order),
    }
}

fn cull_group(
    device: &Device,
    layouts: &Layouts,
    markers: &wgpu::Buffer,
    opaque_order: &wgpu::Buffer,
    layer_order: &wgpu::Buffer,
    args: &MarkerArgs,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
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
                binding: 5,
                resource: args.opaque.buffer().as_entire_binding(),
            },
            BindGroupEntry {
                binding: 6,
                resource: args.layer.buffer().as_entire_binding(),
            },
        ],
    })
}
