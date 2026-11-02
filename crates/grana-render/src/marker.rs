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
use crate::target::SCENE_FORMAT;
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
    include_str!("shader/marker.wgsl")
);
const SECTION_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/marker_section.wgsl")
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
}

impl Hash for Marker {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.center_radius.map(f32::to_bits).hash(state);
        self.color_alpha.map(f32::to_bits).hash(state);
    }
}

pub(crate) struct MarkerPasses {
    markers: Table<Marker>,
    volume_order: Table<u32>,
    behind_order: Table<u32>,
    front_order: Table<u32>,
    volume_group: BindGroup,
    behind_group: BindGroup,
    front_group: BindGroup,
    volume_cull_group: BindGroup,
    behind_cull_group: BindGroup,
    front_cull_group: BindGroup,
    volume_args: InstanceCommands,
    behind_args: InstanceCommands,
    front_args: InstanceCommands,
    volume_cull: ComputePipeline,
    behind_cull: ComputePipeline,
    front_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    section_pipeline: RenderPipeline,
    count: u32,
}

impl MarkerPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let markers = Table::new(device, "grana.markers");
        let volume_order = Table::new(device, "grana.marker.volume.order");
        let behind_order = Table::new(device, "grana.marker.behind.order");
        let front_order = Table::new(device, "grana.marker.front.order");
        let volume_args = InstanceCommands::new(device, "grana.marker.volume.args");
        let behind_args = InstanceCommands::new(device, "grana.marker.behind.args");
        let front_args = InstanceCommands::new(device, "grana.marker.front.args");
        let groups = groups(
            device,
            layouts,
            markers.buffer(),
            [
                volume_order.buffer(),
                behind_order.buffer(),
                front_order.buffer(),
            ],
            [
                volume_args.buffer(),
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
            depth_stencil: Some(cut::pass_state()),
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
        Self {
            markers,
            volume_order,
            behind_order,
            front_order,
            volume_group: groups.volume_render,
            behind_group: groups.behind_render,
            front_group: groups.front_render,
            volume_cull_group: groups.volume_cull,
            behind_cull_group: groups.behind_cull,
            front_cull_group: groups.front_cull,
            volume_args,
            behind_args,
            front_args,
            volume_cull,
            behind_cull,
            front_cull,
            volume_pipeline,
            section_pipeline,
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
        stale |= self.volume_order.reserve(device, markers.len());
        stale |= self.behind_order.reserve(device, markers.len());
        stale |= self.front_order.reserve(device, markers.len());
        if stale {
            let groups = groups(
                device,
                layouts,
                self.markers.buffer(),
                [
                    self.volume_order.buffer(),
                    self.behind_order.buffer(),
                    self.front_order.buffer(),
                ],
                [
                    self.volume_args.buffer(),
                    self.behind_args.buffer(),
                    self.front_args.buffer(),
                ],
            );
            self.volume_group = groups.volume_render;
            self.behind_group = groups.behind_render;
            self.front_group = groups.front_render;
            self.volume_cull_group = groups.volume_cull;
            self.behind_cull_group = groups.behind_cull;
            self.front_cull_group = groups.front_cull;
        }
        self.volume_args.reset(queue, 6);
        self.behind_args.reset(queue, 6);
        self.front_args.reset(queue, 6);
    }

    pub fn cull_volume(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull(
            encoder,
            scene,
            &self.volume_cull,
            &self.volume_cull_group,
            "grana.marker.cull.volume.pass",
        );
    }

    pub fn cull_section(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        self.cull(
            encoder,
            scene,
            &self.behind_cull,
            &self.behind_cull_group,
            "grana.marker.cull.section.behind.pass",
        );
        self.cull(
            encoder,
            scene,
            &self.front_cull,
            &self.front_cull_group,
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
        pass.set_bind_group(1, &self.volume_group, &[]);
        pass.draw_indirect(self.volume_args.buffer(), 0);
    }

    pub fn draw_section_behind<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section(pass, scene, &self.behind_group, &self.behind_args);
    }

    pub fn draw_section_front<'a>(&'a self, pass: &mut RenderPass<'a>, scene: &'a BindGroup) {
        self.draw_section(pass, scene, &self.front_group, &self.front_args);
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
    volume_render: BindGroup,
    behind_render: BindGroup,
    front_render: BindGroup,
    volume_cull: BindGroup,
    behind_cull: BindGroup,
    front_cull: BindGroup,
}

fn groups(
    device: &Device,
    layouts: &Layouts,
    markers: &wgpu::Buffer,
    orders: [&wgpu::Buffer; 3],
    args: [&wgpu::Buffer; 3],
) -> Groups {
    let [volume_order, behind_order, front_order] = orders;
    let [volume_args, behind_args, front_args] = args;
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
    let cull = |label: &str, order: &wgpu::Buffer, args: &wgpu::Buffer| {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.marker_cull,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: markers.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: order.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: args.as_entire_binding(),
                },
            ],
        })
    };
    Groups {
        volume_render: render("grana.marker.volume.render.group", volume_order),
        behind_render: render("grana.marker.behind.render.group", behind_order),
        front_render: render("grana.marker.front.render.group", front_order),
        volume_cull: cull("grana.marker.cull.volume.group", volume_order, volume_args),
        behind_cull: cull("grana.marker.cull.behind.group", behind_order, behind_args),
        front_cull: cull("grana.marker.cull.front.group", front_order, front_args),
    }
}
