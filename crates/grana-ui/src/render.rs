use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};
use wgpu::{FilterMode, Operations as PassOperations};

use crate::atlas::Atlas;
use crate::paint::Instance;

const BLUR_SHADER: &str = include_str!("shader/blur.wgsl");
const UI_SHADER: &str = include_str!("shader/ui.wgsl");
const MIP_LEVELS: u32 = 6;
const MINIMUM_INSTANCES: usize = 512;

const PREMULTIPLIED: BlendState = BlendState {
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
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    scale: f32,
    time: f32,
}

struct Layers {
    full: TextureView,
    levels: Vec<TextureView>,
}

impl Layers {
    fn new(device: &Device, label: &str, size: [u32; 2], format: TextureFormat) -> Self {
        let texture = create_target(device, label, size, format, MIP_LEVELS);
        let full = texture.create_view(&TextureViewDescriptor::default());
        let levels = (0..texture.mip_level_count())
            .map(|level| {
                texture.create_view(&TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self { full, levels }
    }
}

struct Surface {
    texture: Texture,
    view: TextureView,
}

pub struct Renderer {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    color_format: TextureFormat,
    size: [u32; 2],
    scene: Option<Surface>,
    color: Option<Surface>,
    backdrop: Option<Layers>,
    overlay: Option<Layers>,
    downsample: RenderPipeline,
    blit: RenderPipeline,
    blit_linear: RenderPipeline,
    ui: RenderPipeline,
    figure: RenderPipeline,
    blur_layout: BindGroupLayout,
    ui_layout: BindGroupLayout,
    blit_layout: BindGroupLayout,
    sampler: Sampler,
    uniforms: Buffer,
    base: Buffer,
    base_capacity: usize,
    overlay_instances: Buffer,
    overlay_capacity: usize,
    time: f32,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let color_format = unorm(format);
        let blur_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.ui.blur.layout"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let ui_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.ui.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(2),
                sampler_entry(3),
                texture_entry(4),
                sampler_entry(5),
                texture_entry(6),
                sampler_entry(7),
            ],
        });
        let blit_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.ui.blit.layout"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.ui.blur.pipeline_layout"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let ui_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.ui.pipeline_layout"),
            bind_group_layouts: &[Some(&ui_layout)],
            immediate_size: 0,
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.ui.blit.pipeline_layout"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let blur_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ui.blur"),
            source: ShaderSource::Wgsl(BLUR_SHADER.into()),
        });
        let ui_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ui"),
            source: ShaderSource::Wgsl(UI_SHADER.into()),
        });
        let downsample = fullscreen_pipeline(
            device,
            "grana.ui.downsample",
            &blur_pipeline_layout,
            &blur_shader,
            "fs_downsample",
            color_format,
            None,
        );
        let blit = fullscreen_pipeline(
            device,
            "grana.ui.blit",
            &blit_pipeline_layout,
            &blur_shader,
            "fs_blit",
            format,
            None,
        );
        let blit_linear = fullscreen_pipeline(
            device,
            "grana.ui.blit.linear",
            &blit_pipeline_layout,
            &blur_shader,
            "fs_blit_linear",
            format,
            None,
        );
        let ui = instance_pipeline(
            device,
            "grana.ui.instances",
            &ui_pipeline_layout,
            &ui_shader,
            color_format,
        );
        let figure = instance_pipeline(
            device,
            "grana.ui.figure",
            &ui_pipeline_layout,
            &ui_shader,
            format,
        );
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.ui.sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            color_format,
            size: [0, 0],
            scene: None,
            color: None,
            backdrop: None,
            overlay: None,
            downsample,
            blit,
            blit_linear,
            ui,
            figure,
            blur_layout,
            ui_layout,
            blit_layout,
            sampler,
            uniforms,
            base: instance_buffer(device, "grana.ui.instances.base", MINIMUM_INSTANCES),
            base_capacity: MINIMUM_INSTANCES,
            overlay_instances: instance_buffer(
                device,
                "grana.ui.instances.overlay",
                MINIMUM_INSTANCES,
            ),
            overlay_capacity: MINIMUM_INSTANCES,
            time: 0.0,
        }
    }

    pub fn color_format(&self) -> TextureFormat {
        self.color_format
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if size == self.size {
            return;
        }
        self.size = size;
        let device = self.device.clone();
        self.scene = Some(target(&device, "grana.ui.scene", size, self.color_format));
        self.color = Some(target(&device, "grana.ui.color", size, self.color_format));
        let backdrop = [half(size[0]), half(size[1])];
        self.backdrop = Some(Layers::new(
            &device,
            "grana.ui.backdrop",
            backdrop,
            self.color_format,
        ));
        self.overlay = Some(Layers::new(
            &device,
            "grana.ui.backdrop.overlay",
            backdrop,
            self.color_format,
        ));
    }

    pub fn scene_view(&self) -> &TextureView {
        &self.scene.as_ref().expect("the renderer is sized").view
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn read_color(&mut self) -> (Vec<u8>, [u32; 2], TextureFormat) {
        self.read_texture(true)
    }

    fn read_texture(&mut self, color_layer: bool) -> (Vec<u8>, [u32; 2], TextureFormat) {
        let surface = if color_layer {
            self.color.as_ref().expect("the renderer is sized")
        } else {
            self.scene.as_ref().expect("the renderer is sized")
        };
        let color = surface;
        let [width, height] = self.size;
        let padded = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.readback"),
            size: u64::from(padded * height),
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.ui.readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &color.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finished reading the interface");
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .expect("the interface readback is mapped");
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&data[start..start + (width * 4) as usize]);
        }
        drop(data);
        buffer.unmap();
        (pixels, [width, height], self.color_format)
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn write_uniforms(&self, scale: f32, time: f32) {
        let uniforms = Uniforms {
            viewport: [self.size[0] as f32, self.size[1] as f32],
            scale,
            time,
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    fn upload(&mut self, base: &[Instance], overlay: &[Instance]) {
        if self.base_capacity < base.len() {
            self.base_capacity = base.len().next_power_of_two().max(MINIMUM_INSTANCES);
            self.base =
                instance_buffer(&self.device, "grana.ui.instances.base", self.base_capacity);
        }
        if self.overlay_capacity < overlay.len() {
            self.overlay_capacity = overlay.len().next_power_of_two().max(MINIMUM_INSTANCES);
            self.overlay_instances = instance_buffer(
                &self.device,
                "grana.ui.instances.overlay",
                self.overlay_capacity,
            );
        }
        if !base.is_empty() {
            self.queue
                .write_buffer(&self.base, 0, bytemuck::cast_slice(base));
        }
        if !overlay.is_empty() {
            self.queue
                .write_buffer(&self.overlay_instances, 0, bytemuck::cast_slice(overlay));
        }
    }

    pub fn paint(
        &mut self,
        encoder: &mut CommandEncoder,
        target_view: &TextureView,
        atlas: &Atlas,
        base: &[Instance],
        overlay: &[Instance],
    ) {
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        self.upload(base, overlay);
        let Some(color) = self.color.as_ref() else {
            return;
        };
        let Some(backdrop) = self.backdrop.as_ref() else {
            return;
        };
        let Some(overlay_backdrop) = self.overlay.as_ref() else {
            return;
        };
        let scene = &self.scene.as_ref().expect("the renderer is sized").view;
        self.blur(encoder, scene, backdrop);
        self.layer(
            encoder,
            &color.view,
            backdrop,
            scene,
            &self.base,
            base.len(),
            atlas,
            &self.ui,
            false,
        );
        if !overlay.is_empty() {
            self.blur(encoder, &color.view, overlay_backdrop);
            self.layer(
                encoder,
                &color.view,
                overlay_backdrop,
                scene,
                &self.overlay_instances,
                overlay.len(),
                atlas,
                &self.ui,
                true,
            );
        }
        let group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ui.blit.group"),
            layout: &self.blit_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color.view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let pipeline = if self.format.is_srgb() {
            &self.blit_linear
        } else {
            &self.blit
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.blit.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: LoadOp::Clear(wgpu::Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn paint_figure(
        &self,
        encoder: &mut CommandEncoder,
        target_view: &TextureView,
        size: [u32; 2],
        scale: f32,
        atlas: &Atlas,
        instances: &[Instance],
    ) {
        if instances.is_empty() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(backdrop) = self.backdrop.as_ref() else {
            return;
        };
        let buffer = instance_buffer(&self.device, "grana.ui.instances.figure", instances.len());
        self.queue
            .write_buffer(&buffer, 0, bytemuck::cast_slice(instances));
        let uniforms = Uniforms {
            viewport: [size[0] as f32, size[1] as f32],
            scale,
            time: self.time,
        };
        let uniform_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.figure.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let group = self.bind_group(atlas, &backdrop.full, &scene.view, &buffer, &uniform_buffer);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.figure.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.figure);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..6, 0..instances.len() as u32);
    }

    fn blur(&self, encoder: &mut CommandEncoder, source: &TextureView, layers: &Layers) {
        let mut previous: &TextureView = source;
        for level in &layers.levels {
            let group = self.device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.ui.blur.group"),
                layout: &self.blur_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(previous),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.ui.blur.pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: level,
                    depth_slice: None,
                    resolve_target: None,
                    ops: PassOperations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.downsample);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
            previous = level;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layer(
        &self,
        encoder: &mut CommandEncoder,
        target_view: &TextureView,
        backdrop: &Layers,
        scene: &TextureView,
        instances: &Buffer,
        count: usize,
        atlas: &Atlas,
        pipeline: &RenderPipeline,
        load: bool,
    ) {
        if count == 0 {
            return;
        }
        let group = self.bind_group(atlas, &backdrop.full, scene, instances, &self.uniforms);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: if load {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(wgpu::Color::BLACK)
                    },
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    fn bind_group(
        &self,
        atlas: &Atlas,
        backdrop: &TextureView,
        scene: &TextureView,
        instances: &Buffer,
        uniforms: &Buffer,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ui.group"),
            layout: &self.ui_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(backdrop),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(scene),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(atlas.view()),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: BindingResource::Sampler(atlas.sampler()),
                },
            ],
        })
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Sampler(SamplerBindingType::Filtering),
        count: None,
    }
}

fn instance_buffer(device: &Device, label: &str, capacity: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size: (capacity * size_of::<Instance>()) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn unorm(format: TextureFormat) -> TextureFormat {
    match format {
        TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8Unorm,
        TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8Unorm,
        other => other,
    }
}

fn half(value: u32) -> u32 {
    (value / 2).max(1)
}

fn create_target(
    device: &Device,
    label: &str,
    size: [u32; 2],
    format: TextureFormat,
    mip_levels: u32,
) -> Texture {
    let maximum = 32 - size[0].max(size[1]).max(1).leading_zeros();
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: mip_levels.min(maximum).max(1),
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn target(device: &Device, label: &str, size: [u32; 2], format: TextureFormat) -> Surface {
    let texture = create_target(device, label, size, format, 1);
    let view = texture.create_view(&TextureViewDescriptor::default());
    Surface { texture, view }
}

fn fullscreen_pipeline(
    device: &Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment: &str,
    format: TextureFormat,
    blend: Option<BlendState>,
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format,
                blend,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn instance_pipeline(
    device: &Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: TextureFormat,
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(PREMULTIPLIED),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
