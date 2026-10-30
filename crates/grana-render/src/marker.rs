use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, DepthBiasState,
    DepthStencilState, Device, FragmentState, LoadOp, MultisampleState, Operations,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StencilState,
    StoreOp, TextureView, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};

use crate::Camera;
use crate::params::Shading;
use crate::target::{DEPTH_FORMAT, SCENE_FORMAT};
use crate::transfer::srgb_to_linear;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Marker {
    center: [f32; 3],
    radius: f32,
    color: [f32; 3],
    alpha: f32,
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
            center,
            radius,
            color: [red, green, blue],
            alpha,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VolumeUniforms {
    view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    shading: [f32; 4],
    viewport: [f32; 4],
}

const _: () = {
    assert!(size_of::<Marker>() == 32);
    assert!(size_of::<VolumeUniforms>() == 176);
    assert!(std::mem::offset_of!(VolumeUniforms, viewport) == 160);
};

const VOLUME_SHADER: &str = include_str!("marker.wgsl");
const SECTION_SHADER: &str = include_str!("section_marker.wgsl");
const INITIAL_CAPACITY: usize = 64;

pub(crate) struct VolumeFrame<'a> {
    pub target: &'a TextureView,
    pub depth: &'a TextureView,
    pub region: [u32; 4],
    pub camera: &'a Camera,
    pub shading: Shading,
    pub scene_radius: f32,
}

pub(crate) struct SectionFrame<'a> {
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub uniforms: &'a BindGroup,
}

pub(crate) struct Markers {
    instances: Buffer,
    capacity: usize,
    count: usize,
    volume_pipeline: RenderPipeline,
    volume_uniforms: Buffer,
    volume_group: BindGroup,
    section_pipeline: RenderPipeline,
}

impl Markers {
    pub fn new(device: &Device, frame_layout: &BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.marker.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let volume_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.marker.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let section_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.marker.section.pipeline_layout"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let volume_shader = shader(device, "grana.marker", VOLUME_SHADER);
        let section_shader = shader(device, "grana.marker.section", SECTION_SHADER);
        let volume_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker"),
            layout: Some(&volume_layout),
            vertex: VertexState {
                module: &volume_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(instance_layout())],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Less),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &volume_shader,
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
            layout: Some(&section_layout),
            vertex: VertexState {
                module: &section_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(instance_layout())],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &section_shader,
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
        let volume_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.marker.uniforms"),
            size: size_of::<VolumeUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let volume_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.marker.group"),
            layout: &layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: volume_uniforms.as_entire_binding(),
            }],
        });
        let instances = instance_buffer(device, INITIAL_CAPACITY);
        Self {
            instances,
            capacity: INITIAL_CAPACITY,
            count: 0,
            volume_pipeline,
            volume_uniforms,
            volume_group,
            section_pipeline,
        }
    }

    pub fn prepare(&mut self, device: &Device, queue: &Queue, markers: &[Marker]) {
        if markers.len() > self.capacity {
            self.capacity = markers.len().next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        self.count = markers.len();
        if !markers.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(markers));
        }
    }

    pub fn draw_volume(&self, encoder: &mut CommandEncoder, queue: &Queue, frame: VolumeFrame<'_>) {
        let width = frame.region[2].max(1);
        let height = frame.region[3].max(1);
        let aspect = width as f32 / height as f32;
        let eye = frame.camera.eye();
        let view_projection = frame.camera.view_projection(aspect, frame.scene_radius);
        let pixel_scale = 0.5 * height as f32 / (0.5 * frame.camera.fov_y()).tan();
        let uniforms = VolumeUniforms {
            view_projection: view_projection.to_cols_array_2d(),
            eye: eye.extend(0.0).to_array(),
            light: frame.camera.light_direction().extend(0.0).to_array(),
            right: frame.camera.right().extend(0.0).to_array(),
            up: frame.camera.up().extend(0.0).to_array(),
            forward: frame.camera.forward().extend(0.0).to_array(),
            shading: [
                frame.shading.ambient,
                frame.shading.diffuse,
                frame.shading.specular,
                frame.shading.shininess,
            ],
            viewport: [pixel_scale, height as f32, 0.0, 0.0],
        };
        queue.write_buffer(&self.volume_uniforms, 0, bytemuck::bytes_of(&uniforms));
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: frame.depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(
            frame.region[0] as f32,
            frame.region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(frame.region[0], frame.region[1], width, height);
        pass.set_pipeline(&self.volume_pipeline);
        pass.set_bind_group(0, &self.volume_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.count as u32);
    }

    pub fn draw_section(&self, encoder: &mut CommandEncoder, frame: SectionFrame<'_>) {
        if self.count == 0 {
            return;
        }
        let width = frame.region[2].max(1);
        let height = frame.region[3].max(1);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.marker.section.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: frame.target,
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
            frame.region[0] as f32,
            frame.region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(frame.region[0], frame.region[1], width, height);
        pass.set_pipeline(&self.section_pipeline);
        pass.set_bind_group(0, frame.uniforms, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.count as u32);
    }
}

fn shader(device: &Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    })
}

fn instance_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: size_of::<Marker>() as u64,
        step_mode: VertexStepMode::Instance,
        attributes: &[
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 16,
                shader_location: 1,
            },
        ],
    }
}

fn instance_buffer(device: &Device, capacity: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.marker.instances"),
        size: (capacity * size_of::<Marker>()) as u64,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
