use bytemuck::{Pod, Zeroable};
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
use crate::element::Layer;
use crate::image::ImageAtlas;
use crate::paint::{FLAG_BACKDROP, Instance, LayerBuffer};
use grana_gpu::layout;

const BLIT_SHADER: &str = include_str!("shader/blit.wgsl");
const UI_SHADER: &str = include_str!("shader/ui.wgsl");
const MINIMUM_INSTANCES: usize = 512;
const MAX_BLUR_MIPS: u32 = 5;
const OVERLAY_SLOT: usize = Layer::ALL.len();
const SLOTS: usize = Layer::ALL.len() + 1;

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

struct BlurPyramid {
    sampled: TextureView,
    mips: Vec<TextureView>,
}

impl BlurPyramid {
    fn new(device: &Device, size: [u32; 2], format: TextureFormat) -> Self {
        let width = (size[0] / 2).max(1);
        let height = (size[1] / 2).max(1);
        let mip_count = (width.max(height).ilog2() + 1).clamp(1, MAX_BLUR_MIPS);
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.ui.backdrop"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sampled = texture.create_view(&TextureViewDescriptor::default());
        let mips = (0..mip_count)
            .map(|level| {
                texture.create_view(&TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self { sampled, mips }
    }
}

pub struct Figure<'a> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub scale: f32,
    pub instances: &'a [Instance],
}

#[derive(Clone, Copy)]
pub(crate) struct Atlases<'a> {
    pub glyphs: &'a Atlas,
    pub images: &'a ImageAtlas,
}

pub(crate) struct PaintFrame<'a> {
    pub target: &'a TextureView,
    pub scene: Option<&'a TextureView>,
    pub scene_changed: bool,
    pub reset: bool,
    pub atlases: Atlases<'a>,
    pub content: &'a LayerBuffer,
    pub chrome: &'a LayerBuffer,
    pub popup: &'a LayerBuffer,
    pub hud: &'a LayerBuffer,
    pub overlay: &'a LayerBuffer,
}

struct PaintTarget<'a> {
    target: &'a TextureView,
    scene: &'a TextureView,
    backdrop: &'a TextureView,
    load: LoadOp<wgpu::Color>,
}

struct InstanceBuffer {
    buffer: Buffer,
    capacity: usize,
}

impl InstanceBuffer {
    fn new(device: &Device, label: &str) -> Self {
        Self {
            buffer: instance_buffer(device, label, MINIMUM_INSTANCES),
            capacity: MINIMUM_INSTANCES,
        }
    }

    fn upload(&mut self, device: &Device, queue: &Queue, label: &str, layer: &LayerBuffer) {
        if layer.material.is_empty() {
            return;
        }
        let mut grown = false;
        if self.capacity < layer.material.len() {
            self.capacity = layer
                .material
                .len()
                .next_power_of_two()
                .max(MINIMUM_INSTANCES);
            self.buffer = instance_buffer(device, label, self.capacity);
            grown = true;
        }
        let total = layer.material.len();
        match layer.dirty {
            Some((start, end)) if !grown && end > start && end - start < total => {
                queue.write_buffer(
                    &self.buffer,
                    (start * size_of::<Instance>()) as u64,
                    bytemuck::cast_slice(&layer.material[start..end]),
                );
            }
            _ => {
                queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&layer.material));
            }
        }
    }
}

struct Buffers {
    buffers: [InstanceBuffer; SLOTS],
}

impl Buffers {
    fn new(device: &Device) -> Self {
        Self {
            buffers: std::array::from_fn(|index| {
                let label = match index {
                    0 => "grana.ui.instances.content",
                    1 => "grana.ui.instances.chrome",
                    2 => "grana.ui.instances.popup",
                    3 => "grana.ui.instances.hud",
                    _ => "grana.ui.instances.overlay",
                };
                InstanceBuffer::new(device, label)
            }),
        }
    }
}

pub struct Renderer {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    encoded: TextureFormat,
    size: [u32; 2],
    scale: f32,
    canvas: Option<Surface>,
    backdrop: Option<BlurPyramid>,
    dummy: TextureView,
    present_encoded: RenderPipeline,
    present_linear: RenderPipeline,
    downsample: RenderPipeline,
    ui: RenderPipeline,
    figure: RenderPipeline,
    ui_layout: BindGroupLayout,
    blit_layout: BindGroupLayout,
    sampler: Sampler,
    uniforms: Buffer,
    buffers: Buffers,
    figure_instances: Buffer,
    figure_capacity: usize,
    figure_uniforms: Buffer,
    revisions: [u64; SLOTS],
    uniform_state: Option<(f32, [u32; 2])>,
    canvas_generation: u64,
    blit_cache: Option<(u64, BindGroup)>,
    content_blur: bool,
    interface_blur: bool,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let encoded = srgb(format);
        let linear = raw(encoded);
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
                texture_entry(8),
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
        let downsample = fullscreen_pipeline(
            device,
            "grana.ui.downsample",
            &blit_pipeline_layout,
            &blit_shader,
            "fs_downsample",
            linear,
        );
        let ui = instance_pipeline(
            device,
            "grana.ui.instances",
            &ui_pipeline_layout,
            &ui_shader,
            linear,
            PREMULTIPLIED,
        );
        let figure = instance_pipeline(
            device,
            "grana.ui.figure",
            &ui_pipeline_layout,
            &ui_shader,
            format,
            PREMULTIPLIED,
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
            canvas: None,
            backdrop: None,
            dummy: dummy(device),
            present_encoded,
            present_linear,
            downsample,
            ui,
            figure,
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
            revisions: [u64::MAX; SLOTS],
            uniform_state: None,
            canvas_generation: 0,
            blit_cache: None,
            content_blur: false,
            interface_blur: false,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        if size == self.size {
            return false;
        }
        self.size = size;
        let device = self.device.clone();
        self.canvas = Some(Surface::new(&device, "grana.ui.canvas", size, self.encoded));
        self.backdrop = Some(BlurPyramid::new(&device, size, raw(self.encoded)));
        self.canvas_generation = self.canvas_generation.wrapping_add(1);
        self.blit_cache = None;
        self.content_blur = false;
        self.interface_blur = false;
        true
    }

    pub fn read_color(&mut self) -> (Vec<u8>, [u32; 2], TextureFormat) {
        let surface = self.canvas.as_ref().expect("the renderer is sized");
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
        if self.uniform_state == Some((scale, self.size)) {
            return;
        }
        self.uniform_state = Some((scale, self.size));
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
            scene,
            scene_changed,
            reset,
            atlases,
            content,
            chrome,
            popup,
            hud,
            overlay,
        } = frame;
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        let scene = scene.unwrap_or(&self.dummy);
        let canvas = self.canvas.as_ref().expect("the renderer is sized");
        let canvas_linear = canvas.linear.clone();
        let canvas_encoded = canvas.encoded.clone();
        let backdrop = self.backdrop.as_ref().expect("the renderer is sized");
        let backdrop_sampled = backdrop.sampled.clone();

        let layers = [content, chrome, popup, hud, overlay];
        let changed: [bool; SLOTS] =
            std::array::from_fn(|index| self.revisions[index] != layers[index].revision);
        let reupload = changed.iter().any(|changed| *changed);
        let content_refresh = reset || scene_changed || changed[Layer::Content.index()];
        let interface_refresh = content_refresh
            || changed[Layer::Chrome.index()]
            || changed[Layer::Popup.index()]
            || changed[Layer::Hud.index()];
        if reupload {
            for index in 0..SLOTS {
                if !changed[index] {
                    continue;
                }
                self.buffers.buffers[index].upload(
                    &self.device,
                    &self.queue,
                    "grana.ui.instances",
                    layers[index],
                );
                self.revisions[index] = layers[index].revision;
            }
        }
        if reupload || reset || scene_changed {
            clear(encoder, &canvas_linear);
            self.paint_material(
                encoder,
                atlases,
                &PaintTarget {
                    target: &canvas_linear,
                    scene,
                    backdrop: &self.dummy,
                    load: LoadOp::Load,
                },
                &self.buffers.buffers[Layer::Content.index()].buffer,
                content.material.len(),
            );
            if has_backdrop(&chrome.material) {
                if content_refresh || !self.content_blur {
                    self.update_backdrop(encoder, &canvas_linear);
                    self.content_blur = true;
                }
            } else {
                self.content_blur = false;
            }
            self.paint_material(
                encoder,
                atlases,
                &PaintTarget {
                    target: &canvas_linear,
                    scene,
                    backdrop: &backdrop_sampled,
                    load: LoadOp::Load,
                },
                &self.buffers.buffers[Layer::Chrome.index()].buffer,
                chrome.material.len(),
            );
            if has_backdrop(&popup.material) || has_backdrop(&hud.material) {
                if interface_refresh || !self.interface_blur {
                    self.update_backdrop(encoder, &canvas_linear);
                    self.interface_blur = true;
                }
            } else {
                self.interface_blur = false;
            }
            self.paint_material(
                encoder,
                atlases,
                &PaintTarget {
                    target: &canvas_linear,
                    scene,
                    backdrop: &backdrop_sampled,
                    load: LoadOp::Load,
                },
                &self.buffers.buffers[Layer::Popup.index()].buffer,
                popup.material.len(),
            );
            self.paint_material(
                encoder,
                atlases,
                &PaintTarget {
                    target: &canvas_linear,
                    scene,
                    backdrop: &backdrop_sampled,
                    load: LoadOp::Load,
                },
                &self.buffers.buffers[Layer::Hud.index()].buffer,
                hud.material.len(),
            );
            self.paint_material(
                encoder,
                atlases,
                &PaintTarget {
                    target: &canvas_linear,
                    scene,
                    backdrop: &backdrop_sampled,
                    load: LoadOp::Load,
                },
                &self.buffers.buffers[OVERLAY_SLOT].buffer,
                overlay.material.len(),
            );
        }

        if self
            .blit_cache
            .as_ref()
            .is_none_or(|(generation, _)| *generation != self.canvas_generation)
        {
            let group = self.blit_group(&canvas_encoded);
            self.blit_cache = Some((self.canvas_generation, group));
        }
        let group = &self
            .blit_cache
            .as_ref()
            .expect("the blit group is cached")
            .1;
        let pipeline = if self.format.is_srgb() {
            &self.present_linear
        } else {
            &self.present_encoded
        };
        fullscreen(
            encoder,
            pipeline,
            group,
            target_view,
            LoadOp::Clear(wgpu::Color::BLACK),
        );
    }

    fn update_backdrop(&self, encoder: &mut CommandEncoder, source: &TextureView) {
        let Some(backdrop) = self.backdrop.as_ref() else {
            return;
        };
        for (index, target) in backdrop.mips.iter().enumerate() {
            let input = if index == 0 {
                source
            } else {
                &backdrop.mips[index - 1]
            };
            let group = self.blit_group(input);
            fullscreen(
                encoder,
                &self.downsample,
                &group,
                target,
                LoadOp::Clear(wgpu::Color::BLACK),
            );
        }
    }

    fn paint_material(
        &self,
        encoder: &mut CommandEncoder,
        atlases: Atlases<'_>,
        target: &PaintTarget<'_>,
        buffer: &Buffer,
        count: usize,
    ) {
        if count == 0 {
            return;
        }
        let group = self.ui_group(
            atlases,
            target.scene,
            target.backdrop,
            buffer,
            &self.uniforms,
        );
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.target,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: target.load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.ui);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    pub fn paint_figure(
        &mut self,
        encoder: &mut CommandEncoder,
        figure: Figure<'_>,
        atlas: &Atlas,
        images: &ImageAtlas,
    ) {
        let Figure {
            target: target_view,
            size,
            scale,
            instances,
        } = figure;
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
        self.queue
            .write_buffer(&self.figure_instances, 0, bytemuck::cast_slice(instances));
        let uniforms = Uniforms {
            viewport: [size[0] as f32, size[1] as f32],
            scale,
            padding: 0.0,
        };
        self.queue
            .write_buffer(&self.figure_uniforms, 0, bytemuck::bytes_of(&uniforms));
        let scene = &self.dummy;
        let group = self.ui_group(
            Atlases {
                glyphs: atlas,
                images,
            },
            scene,
            scene,
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
        atlases: Atlases<'_>,
        scene: &TextureView,
        backdrop: &TextureView,
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
                    resource: BindingResource::TextureView(atlases.glyphs.view()),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::Sampler(atlases.glyphs.sampler()),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(atlases.images.view()),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: BindingResource::Sampler(atlases.images.sampler()),
                },
                BindGroupEntry {
                    binding: 8,
                    resource: BindingResource::TextureView(backdrop),
                },
            ],
        })
    }
}

fn has_backdrop(instances: &[Instance]) -> bool {
    instances
        .iter()
        .any(|instance| (instance.meta[1] & FLAG_BACKDROP) != 0)
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

fn dummy(device: &Device) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some("grana.ui.dummy"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default())
}

fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.ui.clear.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: PassOperations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
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
    blend: BlendState,
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
                blend: Some(blend),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
