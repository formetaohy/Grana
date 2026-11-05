use bytemuck::{Pod, Zeroable};
use grana_glass::{Frame as GlassFrame, Renderer as GlassRenderer, Scene as GlassScene};
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent,
    BlendFactor, BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor,
    BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FilterMode,
    FragmentState, LoadOp, MipmapFilterMode, Operations as PassOperations,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerDescriptor,
    ShaderModule, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor, VertexState,
};

use crate::atlas::Atlas;
use crate::glass::FLOATING;
use crate::paint::Instance;
use grana_gpu::Target;
use grana_gpu::layout;

const BLIT_SHADER: &str = include_str!("shader/blit.wgsl");
const UI_SHADER: &str = include_str!("shader/ui.wgsl");
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
    padding: f32,
}

struct Surface {
    texture: Texture,
    linear: TextureView,
    encoded: TextureView,
}

impl Surface {
    fn new(device: &Device, label: &str, size: [u32; 2], encoded: TextureFormat) -> Self {
        let linear = raw(encoded);
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: encoded,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[linear],
        });
        let default = texture.create_view(&TextureViewDescriptor::default());
        let raw_view = texture.create_view(&TextureViewDescriptor {
            format: Some(linear),
            ..Default::default()
        });
        let (encoded_view, linear_view) = if encoded.is_srgb() {
            (default, raw_view)
        } else {
            (raw_view, default)
        };
        Self {
            texture,
            linear: linear_view,
            encoded: encoded_view,
        }
    }
}

pub(crate) struct PaintFrame<'a> {
    pub target: &'a TextureView,
    pub atlas: &'a Atlas,
    pub base: &'a [Instance],
    pub chrome: &'a [Instance],
    pub popup: &'a [Instance],
    pub glass: &'a GlassScene,
    pub floating: bool,
}

struct Layer<'a> {
    target: &'a TextureView,
    scene: &'a TextureView,
    buffer: &'a Buffer,
    instances: &'a [Instance],
    load: LoadOp<wgpu::Color>,
}

struct Buffers {
    base: Buffer,
    base_capacity: usize,
    chrome: Buffer,
    chrome_capacity: usize,
    popup: Buffer,
    popup_capacity: usize,
}

impl Buffers {
    fn new(device: &Device) -> Self {
        Self {
            base: instance_buffer(device, "grana.ui.instances.base", MINIMUM_INSTANCES),
            base_capacity: MINIMUM_INSTANCES,
            chrome: instance_buffer(device, "grana.ui.instances.chrome", MINIMUM_INSTANCES),
            chrome_capacity: MINIMUM_INSTANCES,
            popup: instance_buffer(device, "grana.ui.instances.popup", MINIMUM_INSTANCES),
            popup_capacity: MINIMUM_INSTANCES,
        }
    }

    fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        base: &[Instance],
        chrome: &[Instance],
        popup: &[Instance],
    ) {
        upload_instances(
            device,
            queue,
            "grana.ui.instances.base",
            base,
            &mut self.base,
            &mut self.base_capacity,
        );
        upload_instances(
            device,
            queue,
            "grana.ui.instances.chrome",
            chrome,
            &mut self.chrome,
            &mut self.chrome_capacity,
        );
        upload_instances(
            device,
            queue,
            "grana.ui.instances.popup",
            popup,
            &mut self.popup,
            &mut self.popup_capacity,
        );
    }
}

fn upload_instances(
    device: &Device,
    queue: &Queue,
    label: &str,
    instances: &[Instance],
    buffer: &mut Buffer,
    capacity: &mut usize,
) {
    if instances.is_empty() {
        return;
    }
    if *capacity < instances.len() {
        *capacity = instances.len().next_power_of_two().max(MINIMUM_INSTANCES);
        *buffer = instance_buffer(device, label, *capacity);
    }
    queue.write_buffer(buffer, 0, bytemuck::cast_slice(instances));
}

pub struct Renderer {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    encoded: TextureFormat,
    size: [u32; 2],
    scale: f32,
    scene: Option<Target>,
    back: Option<Surface>,
    front: Option<Surface>,
    blit: RenderPipeline,
    present_encoded: RenderPipeline,
    present_linear: RenderPipeline,
    ui: RenderPipeline,
    figure: RenderPipeline,
    lower: GlassRenderer,
    upper: GlassRenderer,
    ui_layout: BindGroupLayout,
    blit_layout: BindGroupLayout,
    sampler: Sampler,
    uniforms: Buffer,
    buffers: Buffers,
    figure_instances: Buffer,
    figure_capacity: usize,
    figure_uniforms: Buffer,
    floating: bool,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let encoded = srgb(format);
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
            ],
        });
        let blit_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.ui.blit.layout"),
            entries: &[texture_entry(0), sampler_entry(1)],
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
        let blit_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ui.blit"),
            source: ShaderSource::Wgsl(BLIT_SHADER.into()),
        });
        let ui_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.ui"),
            source: ShaderSource::Wgsl(UI_SHADER.into()),
        });
        let blit = fullscreen_pipeline(
            device,
            "grana.ui.blit",
            &blit_pipeline_layout,
            &blit_shader,
            "fs_blit",
            raw(encoded),
        );
        let present_encoded = fullscreen_pipeline(
            device,
            "grana.ui.present.encoded",
            &blit_pipeline_layout,
            &blit_shader,
            "fs_present_encoded",
            format,
        );
        let present_linear = fullscreen_pipeline(
            device,
            "grana.ui.present.linear",
            &blit_pipeline_layout,
            &blit_shader,
            "fs_present_linear",
            format,
        );
        let ui = instance_pipeline(
            device,
            "grana.ui.instances",
            &ui_pipeline_layout,
            &ui_shader,
            raw(encoded),
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
            mipmap_filter: MipmapFilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            encoded,
            size: [0, 0],
            scale: 1.0,
            scene: None,
            back: None,
            front: None,
            blit,
            present_encoded,
            present_linear,
            ui,
            figure,
            lower: GlassRenderer::new(device, encoded),
            upper: GlassRenderer::new(device, encoded),
            ui_layout,
            blit_layout,
            sampler,
            uniforms,
            buffers: Buffers::new(device),
            figure_instances: instance_buffer(
                device,
                "grana.ui.instances.figure",
                MINIMUM_INSTANCES,
            ),
            figure_capacity: MINIMUM_INSTANCES,
            figure_uniforms: device.create_buffer(&BufferDescriptor {
                label: Some("grana.ui.figure.uniforms"),
                size: size_of::<Uniforms>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            floating: false,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if size == self.size {
            return;
        }
        self.size = size;
        let device = self.device.clone();
        self.scene = Some(Target::new(&device, "grana.ui.scene", size, self.format));
        self.back = Some(Surface::new(&device, "grana.ui.back", size, self.encoded));
        self.front = Some(Surface::new(&device, "grana.ui.front", size, self.encoded));
    }

    pub fn scene_view(&self) -> &TextureView {
        &self.scene.as_ref().expect("the renderer is sized").view
    }

    pub fn read_color(&mut self) -> (Vec<u8>, [u32; 2], TextureFormat) {
        let surface = if self.floating {
            self.back.as_ref().expect("the renderer is sized")
        } else {
            self.front.as_ref().expect("the renderer is sized")
        };
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
            TexelCopyTextureInfo {
                texture: &surface.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
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
        (pixels, [width, height], raw(self.format))
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn write_uniforms(&mut self, scale: f32) {
        self.scale = scale;
        let uniforms = Uniforms {
            viewport: [self.size[0] as f32, self.size[1] as f32],
            scale,
            padding: 0.0,
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    pub(crate) fn paint(&mut self, encoder: &mut CommandEncoder, frame: PaintFrame<'_>) {
        let PaintFrame {
            target: target_view,
            atlas,
            base,
            chrome,
            popup,
            glass,
            floating,
        } = frame;
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        let (Some(scene), Some(back), Some(front)) = (
            self.scene.as_ref().map(|scene| scene.view.clone()),
            self.back.as_ref(),
            self.front.as_ref(),
        ) else {
            return;
        };
        self.floating = floating;
        self.buffers
            .upload(&self.device, &self.queue, base, chrome, popup);
        let scale = self.scale;
        let size = (self.size[0], self.size[1]);
        let scene_group = self.blit_group(&scene);
        fullscreen(
            encoder,
            &self.blit,
            &scene_group,
            &back.linear,
            LoadOp::Clear(wgpu::Color::BLACK),
        );
        self.paint_instances(
            encoder,
            atlas,
            Layer {
                target: &back.linear,
                scene: &scene,
                buffer: &self.buffers.base,
                instances: base,
                load: LoadOp::Load,
            },
        );
        self.lower.render_layers(
            &self.device,
            &self.queue,
            encoder,
            &GlassFrame {
                content: &back.encoded,
                target: &front.encoded,
                size,
                scale,
                content_changed: true,
            },
            glass,
            ..FLOATING,
        );
        self.paint_instances(
            encoder,
            atlas,
            Layer {
                target: &front.linear,
                scene: &scene,
                buffer: &self.buffers.chrome,
                instances: chrome,
                load: LoadOp::Load,
            },
        );
        let present = if floating {
            self.upper.render_layers(
                &self.device,
                &self.queue,
                encoder,
                &GlassFrame {
                    content: &front.encoded,
                    target: &back.encoded,
                    size,
                    scale,
                    content_changed: true,
                },
                glass,
                FLOATING..,
            );
            self.paint_instances(
                encoder,
                atlas,
                Layer {
                    target: &back.linear,
                    scene: &scene,
                    buffer: &self.buffers.popup,
                    instances: popup,
                    load: LoadOp::Load,
                },
            );
            &back.encoded
        } else {
            &front.encoded
        };
        let group = self.blit_group(present);
        let pipeline = if self.format.is_srgb() {
            &self.present_linear
        } else {
            &self.present_encoded
        };
        fullscreen(
            encoder,
            pipeline,
            &group,
            target_view,
            LoadOp::Clear(wgpu::Color::BLACK),
        );
    }

    fn paint_instances(&self, encoder: &mut CommandEncoder, atlas: &Atlas, layer: Layer<'_>) {
        if layer.instances.is_empty() {
            return;
        }
        let group = self.ui_group(atlas, layer.scene, layer.buffer, &self.uniforms);
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: layer.target,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: layer.load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.ui);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..6, 0..layer.instances.len() as u32);
    }

    pub fn paint_figure(
        &mut self,
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
        if self.figure_capacity < instances.len() {
            self.figure_capacity = instances.len().next_power_of_two().max(MINIMUM_INSTANCES);
            self.figure_instances = instance_buffer(
                &self.device,
                "grana.ui.instances.figure",
                self.figure_capacity,
            );
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        self.queue
            .write_buffer(&self.figure_instances, 0, bytemuck::cast_slice(instances));
        let uniforms = Uniforms {
            viewport: [size[0] as f32, size[1] as f32],
            scale,
            padding: 0.0,
        };
        self.queue
            .write_buffer(&self.figure_uniforms, 0, bytemuck::bytes_of(&uniforms));
        let group = self.ui_group(
            atlas,
            &scene.view,
            &self.figure_instances,
            &self.figure_uniforms,
        );
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

    fn blit_group(&self, source: &TextureView) -> BindGroup {
        self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ui.blit.group"),
            layout: &self.blit_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn ui_group(
        &self,
        atlas: &Atlas,
        scene: &TextureView,
        instances: &Buffer,
        uniforms: &Buffer,
    ) -> BindGroup {
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
                    resource: BindingResource::TextureView(scene),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(atlas.view()),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::Sampler(atlas.sampler()),
                },
            ],
        })
    }
}

fn raw(format: TextureFormat) -> TextureFormat {
    match format {
        TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8Unorm,
        TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8Unorm,
        TextureFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
        TextureFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        other => panic!("the interface cannot encode {other:?}"),
    }
}

fn srgb(format: TextureFormat) -> TextureFormat {
    match format {
        TextureFormat::Bgra8Unorm => TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba8Unorm => TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
        other => panic!("the interface cannot encode {other:?}"),
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    layout::texture(binding, ShaderStages::FRAGMENT)
}

fn sampler_entry(binding: u32) -> BindGroupLayoutEntry {
    layout::sampler(binding, ShaderStages::FRAGMENT)
}

fn instance_buffer(device: &Device, label: &str, capacity: usize) -> Buffer {
    grana_gpu::storage_buffer::<Instance>(device, label, capacity)
}

fn fullscreen(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    target: &TextureView,
    load: LoadOp<wgpu::Color>,
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.ui.fullscreen.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: PassOperations {
                load,
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

fn fullscreen_pipeline(
    device: &Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &ShaderModule,
    fragment: &str,
    format: TextureFormat,
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
                blend: None,
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
    shader: &ShaderModule,
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
