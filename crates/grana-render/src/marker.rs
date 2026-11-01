use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable};
use grana_gpu::{InstanceCommands, Table};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BlendState, ColorTargetState, ColorWrites,
    CommandEncoder, CompareFunction, ComputePipeline, ComputePipelineDescriptor, Device,
    FragmentState, LoadOp, MultisampleState, Operations, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, StoreOp,
    TextureView, VertexState,
};

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
    order: Table<u32>,
    render_group: BindGroup,
    volume_cull_group: BindGroup,
    section_cull_group: BindGroup,
    volume_args: InstanceCommands,
    section_args: InstanceCommands,
    volume_cull: ComputePipeline,
    section_cull: ComputePipeline,
    volume_pipeline: RenderPipeline,
    section_pipeline: RenderPipeline,
    count: u32,
}

impl MarkerPasses {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let markers = Table::new(device, "grana.markers");
        let order = Table::new(device, "grana.marker.order");
        let volume_args = InstanceCommands::new(device, "grana.marker.volume.args");
        let section_args = InstanceCommands::new(device, "grana.marker.section.args");
        let (render_group, volume_cull_group, section_cull_group) = groups(
            device,
            layouts,
            markers.buffer(),
            order.buffer(),
            volume_args.buffer(),
            section_args.buffer(),
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
        let section_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.marker.cull.section"),
            layout: Some(&layouts.marker_cull_pipeline),
            module: &cull_module,
            entry_point: Some("marker_cull_section"),
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
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_module,
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
        Self {
            markers,
            order,
            render_group,
            volume_cull_group,
            section_cull_group,
            volume_args,
            section_args,
            volume_cull,
            section_cull,
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
        stale |= self.order.reserve(device, markers.len());
        if stale {
            let groups = groups(
                device,
                layouts,
                self.markers.buffer(),
                self.order.buffer(),
                self.volume_args.buffer(),
                self.section_args.buffer(),
            );
            (
                self.render_group,
                self.volume_cull_group,
                self.section_cull_group,
            ) = groups;
        }
        self.volume_args.reset(queue, 6);
        self.section_args.reset(queue, 6);
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
        pass.set_bind_group(1, &self.volume_cull_group, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
    }

    pub fn cull_section(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.marker.cull.section.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.section_cull);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.section_cull_group, &[]);
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
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.draw_indirect(self.volume_args.buffer(), 0);
    }

    pub fn draw_section(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        region: [u32; 4],
        scene: &BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let width = region[2].max(1);
        let height = region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.section.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
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
        pass.set_pipeline(&self.section_pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, &self.render_group, &[]);
        pass.draw_indirect(self.section_args.buffer(), 0);
    }
}

fn groups(
    device: &Device,
    layouts: &Layouts,
    markers: &wgpu::Buffer,
    order: &wgpu::Buffer,
    volume: &wgpu::Buffer,
    section: &wgpu::Buffer,
) -> (BindGroup, BindGroup, BindGroup) {
    let render = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.marker.render.group"),
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
    });
    let cull = |label: &str, args: &wgpu::Buffer| {
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
    (
        render,
        cull("grana.marker.cull.volume.group", volume),
        cull("grana.marker.cull.section.group", section),
    )
}
