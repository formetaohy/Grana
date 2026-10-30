use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp, MultisampleState,
    Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, TextureView, VertexAttribute, VertexBufferLayout,
    VertexFormat, VertexState, VertexStepMode,
};

use crate::transfer::srgb_to_linear;
use crate::{Camera, RenderParams};

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
struct Uniforms {
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
    assert!(size_of::<Uniforms>() == 176);
    assert!(std::mem::offset_of!(Uniforms, viewport) == 160);
};

const SHADER: &str = concat!(
    include_str!("sampling.wgsl"),
    "\n",
    include_str!("marker.wgsl")
);
const INITIAL_CAPACITY: usize = 64;

pub(crate) struct MarkerFrame<'a> {
    pub encoder: &'a mut CommandEncoder,
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub camera: &'a Camera,
    pub params: &'a RenderParams,
    pub scene_radius: f32,
}

pub(crate) struct Markers {
    pipeline: RenderPipeline,
    uniforms: Buffer,
    group: BindGroup,
    instances: Buffer,
    capacity: usize,
    staging: Vec<Marker>,
}

impl Markers {
    pub fn new(device: &Device, format: wgpu::TextureFormat) -> Self {
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
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.marker.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.marker"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.marker"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
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
                })],
            },
            primitive: PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.marker.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.marker.group"),
            layout: &layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let instances = marker_buffer(device, INITIAL_CAPACITY);
        Self {
            pipeline,
            uniforms,
            group,
            instances,
            capacity: INITIAL_CAPACITY,
            staging: Vec::new(),
        }
    }

    pub fn draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: MarkerFrame<'_>,
        markers: &[Marker],
    ) {
        if markers.is_empty() {
            return;
        }
        let MarkerFrame {
            encoder,
            target,
            region,
            camera,
            params,
            scene_radius,
        } = frame;
        let width = region[2].max(1);
        let height = region[3].max(1);
        let aspect = width as f32 / height as f32;
        self.staging.clear();
        self.staging.extend_from_slice(markers);
        let eye = camera.eye();
        let forward = camera.forward();
        self.staging.sort_by(|left, right| {
            let left = (Vec3::from(left.center) - eye).dot(forward);
            let right = (Vec3::from(right.center) - eye).dot(forward);
            right.total_cmp(&left)
        });
        if self.staging.len() > self.capacity {
            self.capacity = self.staging.len().next_power_of_two();
            self.instances = marker_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.staging));
        let view_projection = camera.view_projection(aspect, scene_radius);
        let right = camera.right();
        let up = camera.up();
        let light = camera.light_direction();
        let pixel_scale = 0.5 * height as f32 / (0.5 * camera.fov_y()).tan();
        let uniforms = Uniforms {
            view_projection: view_projection.to_cols_array_2d(),
            eye: eye.extend(0.0).to_array(),
            light: light.extend(0.0).to_array(),
            right: right.extend(0.0).to_array(),
            up: up.extend(0.0).to_array(),
            forward: forward.extend(0.0).to_array(),
            shading: [
                params.shading.ambient,
                params.shading.diffuse,
                params.shading.specular,
                params.shading.shininess,
            ],
            viewport: [pixel_scale, height as f32, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.staging.len() as u32);
    }
}

fn marker_buffer(device: &Device, capacity: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.marker.instances"),
        size: (capacity * size_of::<Marker>()) as u64,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
