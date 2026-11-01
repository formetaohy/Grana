use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FilterMode, FragmentState,
    LoadOp, MipmapFilterMode, Operations as PassOperations, PipelineLayoutDescriptor,
    PrimitiveState, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
    TextureViewDimension, VertexState,
};

use crate::atlas::Atlas;
use crate::paint::{FLAG_CHROME, GLASS_MARGIN, Instance, KIND_GLASS};

const BLUR_SHADER: &str = include_str!("shader/blur.wgsl");
const UI_SHADER: &str = include_str!("shader/ui.wgsl");
const MAP_BYTES: &[u8] = include_bytes!("../assets/displacement-map.png");
const MINIMUM_INSTANCES: usize = 512;
const BLUR_PASSES: usize = 6;
const BLUR_STRIDE: u64 = 256;
const BLUR_RADIUS: f32 = 4.0;
const BLUR_SATURATION: f32 = 1.4;

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
struct BlurUniforms {
    base: [f32; 4],
    limits: [f32; 4],
    step: [f32; 2],
    radius: f32,
    saturation: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    scale: f32,
    time: f32,
    pointer: [f32; 2],
    padding: [f32; 2],
}

struct Target {
    texture: Texture,
    view: TextureView,
}

struct ElementTarget {
    _texture: Texture,
    view: TextureView,
    size: [u32; 2],
}

enum Segment {
    Batch { start: usize, len: usize },
    Glass { index: usize },
}

pub struct Renderer {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    color_format: TextureFormat,
    size: [u32; 2],
    scale: f32,
    scene: Option<Target>,
    color: Option<Target>,
    backdrop: Option<Target>,
    elements: Vec<ElementTarget>,
    box_pass: RenderPipeline,
    blit: RenderPipeline,
    blit_linear: RenderPipeline,
    ui: RenderPipeline,
    figure: RenderPipeline,
    blur_layout: BindGroupLayout,
    ui_layout: BindGroupLayout,
    blit_layout: BindGroupLayout,
    sampler: Sampler,
    map_sampler: Sampler,
    map: TextureView,
    uniforms: Buffer,
    blur_uniforms: Buffer,
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
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            size_of::<BlurUniforms>() as u64
                        ),
                    },
                    count: None,
                },
            ],
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
                texture_entry(8),
                sampler_entry(9),
                texture_entry(10),
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
        let box_blur = fullscreen_pipeline(
            device,
            "grana.ui.blur.box",
            &blur_pipeline_layout,
            &blur_shader,
            "fs_box",
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
        let blur_uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.blur.uniforms"),
            size: BLUR_STRIDE * BLUR_PASSES as u64 * 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.ui.sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let map_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.ui.map.sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let map = upload_map(device, queue);
        Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            color_format,
            size: [0, 0],
            scale: 1.0,
            scene: None,
            color: None,
            backdrop: None,
            elements: Vec::new(),
            box_pass: box_blur,
            blit,
            blit_linear,
            ui,
            figure,
            blur_layout,
            ui_layout,
            blit_layout,
            sampler,
            map_sampler,
            map,
            uniforms,
            blur_uniforms,
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
        self.backdrop = Some(target(
            &device,
            "grana.ui.backdrop",
            size,
            self.color_format,
        ));
        self.elements.clear();
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
            TexelCopyTextureInfo {
                texture: &color.texture,
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
        (pixels, [width, height], self.color_format)
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn write_uniforms(&mut self, scale: f32, time: f32, pointer: [f32; 2]) {
        self.scale = scale;
        let uniforms = Uniforms {
            viewport: [self.size[0] as f32, self.size[1] as f32],
            scale,
            time,
            pointer,
            padding: [0.0; 2],
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        self.time = time;
        self.write_blur_uniforms(scale);
    }

    fn write_blur_uniforms(&self, scale: f32) {
        let sigma = BLUR_RADIUS * scale;
        let mut window = (sigma * 1.879_97).round().max(1.0) as u32;
        if window.is_multiple_of(2) {
            window += 1;
        }
        let radius = ((window - 1) / 2) as f32;
        let mut blocks = [0u8; BLUR_STRIDE as usize * BLUR_PASSES];
        for pass in 0..BLUR_PASSES {
            let direction = if pass < BLUR_PASSES / 2 {
                [1.0f32, 0.0]
            } else {
                [0.0f32, 1.0]
            };
            let saturation = if pass + 1 == BLUR_PASSES {
                BLUR_SATURATION
            } else {
                1.0
            };
            let block = [
                direction[0].to_ne_bytes(),
                direction[1].to_ne_bytes(),
                radius.to_ne_bytes(),
                saturation.to_ne_bytes(),
            ]
            .concat();
            let start = pass * BLUR_STRIDE as usize;
            blocks[start..start + 16].copy_from_slice(&block);
        }
        self.queue.write_buffer(&self.blur_uniforms, 0, &blocks);
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
        let Some(color) = self.color.as_ref().map(|color| color.view.clone()) else {
            return;
        };
        let Some(scene) = self.scene.as_ref().map(|scene| scene.view.clone()) else {
            return;
        };
        let Some(backdrop) = self
            .backdrop
            .as_ref()
            .map(|backdrop| (backdrop.texture.clone(), backdrop.view.clone()))
        else {
            return;
        };
        let overlay_rects = glass_rects(overlay, self.scale, self.size);
        let chrome_start = base.iter().position(|instance| {
            instance.meta[0] == KIND_GLASS || instance.meta[1] & FLAG_CHROME != 0
        });
        if let Some(start) = chrome_start {
            let content = &base[..start];
            let chrome = &base[start..];
            let chrome_rects = glass_rects(chrome, self.scale, self.size);
            self.write_glass_uniforms(&chrome_rects, &overlay_rects);
            if content.is_empty() {
                clear_target(encoder, &color);
            } else {
                self.paint_layer(
                    encoder,
                    &color,
                    &scene,
                    &scene,
                    &scene,
                    content,
                    0,
                    self.base.clone(),
                    atlas,
                    false,
                    0,
                    &[],
                );
            }
            encoder.copy_texture_to_texture(
                TexelCopyTextureInfo {
                    texture: &self
                        .color
                        .as_ref()
                        .expect("the interface layer is sized")
                        .texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                TexelCopyTextureInfo {
                    texture: &backdrop.0,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                Extent3d {
                    width: self.size[0],
                    height: self.size[1],
                    depth_or_array_layers: 1,
                },
            );
            self.paint_layer(
                encoder,
                &color,
                &backdrop.1,
                &backdrop.1,
                &scene,
                chrome,
                start,
                self.base.clone(),
                atlas,
                true,
                0,
                &chrome_rects,
            );
            if !overlay.is_empty() {
                self.paint_layer(
                    encoder,
                    &color,
                    &color,
                    &backdrop.1,
                    &scene,
                    overlay,
                    0,
                    self.overlay_instances.clone(),
                    atlas,
                    true,
                    chrome_rects.len(),
                    &overlay_rects,
                );
            }
        } else {
            let base_rects = glass_rects(base, self.scale, self.size);
            self.write_glass_uniforms(&base_rects, &overlay_rects);
            self.paint_layer(
                encoder,
                &color,
                &color,
                &scene,
                &scene,
                base,
                0,
                self.base.clone(),
                atlas,
                false,
                0,
                &base_rects,
            );
            if !overlay.is_empty() {
                self.paint_layer(
                    encoder,
                    &color,
                    &color,
                    &scene,
                    &scene,
                    overlay,
                    0,
                    self.overlay_instances.clone(),
                    atlas,
                    true,
                    base_rects.len(),
                    &overlay_rects,
                );
            }
        }
        let group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.ui.blit.group"),
            layout: &self.blit_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color),
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

    fn write_glass_uniforms(&self, base: &[[f32; 4]], overlay: &[[f32; 4]]) {
        let count = (base.len() + overlay.len()).max(1);
        let sigma = BLUR_RADIUS * self.scale;
        let mut window = (sigma * 1.879_97).round().max(1.0) as u32;
        if window.is_multiple_of(2) {
            window += 1;
        }
        let radius = ((window - 1) / 2) as f32;
        let max_offset = self.blur_uniforms.size() as usize;
        let stride = BLUR_STRIDE as usize;
        let mut blocks = vec![0u8; (count * BLUR_PASSES * stride).min(max_offset)];
        for (element, rect) in base.iter().chain(overlay.iter()).enumerate() {
            let width = rect[2].max(1.0);
            let height = rect[3].max(1.0);
            for pass in 0..BLUR_PASSES {
                let direction = if pass < BLUR_PASSES / 2 {
                    [1.0f32, 0.0]
                } else {
                    [0.0f32, 1.0]
                };
                let saturation = if pass + 1 == BLUR_PASSES {
                    BLUR_SATURATION
                } else {
                    1.0
                };
                let viewport = [self.size[0] as f32, self.size[1] as f32];
                let (base, limits, step) = if pass == 0 {
                    (
                        [
                            rect[0] / viewport[0],
                            rect[1] / viewport[1],
                            width / viewport[0],
                            height / viewport[1],
                        ],
                        [
                            (rect[0] + 0.5) / viewport[0],
                            (rect[1] + 0.5) / viewport[1],
                            (rect[0] + width - 0.5) / viewport[0],
                            (rect[1] + height - 0.5) / viewport[1],
                        ],
                        [direction[0] / viewport[0], direction[1] / viewport[1]],
                    )
                } else {
                    (
                        [0.0, 0.0, 1.0, 1.0],
                        [
                            0.5 / width,
                            0.5 / height,
                            1.0 - 0.5 / width,
                            1.0 - 0.5 / height,
                        ],
                        [direction[0] / width, direction[1] / height],
                    )
                };
                let uniforms = BlurUniforms {
                    base,
                    limits,
                    step,
                    radius,
                    saturation,
                };
                let start = (element * BLUR_PASSES + pass) * stride;
                if start + size_of::<BlurUniforms>() <= blocks.len() {
                    blocks[start..start + size_of::<BlurUniforms>()]
                        .copy_from_slice(bytemuck::bytes_of(&uniforms));
                }
            }
        }
        self.queue.write_buffer(&self.blur_uniforms, 0, &blocks);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_layer(
        &mut self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        source: &TextureView,
        sharp: &TextureView,
        scene: &TextureView,
        instances: &[Instance],
        instance_base: usize,
        buffer: Buffer,
        atlas: &Atlas,
        load: bool,
        uniform_base: usize,
        rects: &[[f32; 4]],
    ) {
        if instances.is_empty() {
            return;
        }
        let mut first = !load;
        let mut element = 0usize;
        for segment in segments(instances) {
            match segment {
                Segment::Batch { start, len } => {
                    let group =
                        self.bind_group(atlas, scene, scene, sharp, &buffer, &self.uniforms);
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.ui.pass"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: PassOperations {
                                load: if first {
                                    LoadOp::Clear(wgpu::Color::BLACK)
                                } else {
                                    LoadOp::Load
                                },
                                store: StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    pass.set_pipeline(&self.ui);
                    pass.set_bind_group(0, &group, &[]);
                    pass.draw(
                        0..6,
                        (instance_base + start) as u32..(instance_base + start + len) as u32,
                    );
                    first = false;
                }
                Segment::Glass { index } => {
                    let rect = rects[element];
                    element += 1;
                    let (a, b) = self.element_pair(rect);
                    self.blur_element(encoder, source, a, b, rect, uniform_base + element - 1);
                    let backdrop = self.elements[b].view.clone();
                    let group =
                        self.bind_group(atlas, &backdrop, scene, sharp, &buffer, &self.uniforms);
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.ui.glass.pass"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: PassOperations {
                                load: if first {
                                    LoadOp::Clear(wgpu::Color::BLACK)
                                } else {
                                    LoadOp::Load
                                },
                                store: StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    pass.set_pipeline(&self.ui);
                    pass.set_bind_group(0, &group, &[]);
                    pass.draw(
                        0..6,
                        (instance_base + index) as u32..(instance_base + index) as u32 + 1,
                    );
                    first = false;
                }
            }
        }
    }

    fn element_pair(&mut self, rect: [f32; 4]) -> (usize, usize) {
        let size = [rect[2].max(1.0) as u32, rect[3].max(1.0) as u32];
        for index in (0..self.elements.len().saturating_sub(1)).step_by(2) {
            if self.elements[index].size == size && self.elements[index + 1].size == size {
                return (index, index + 1);
            }
        }
        for _ in 0..2 {
            let created = element_target(
                &self.device,
                "grana.ui.glass.element",
                size,
                self.color_format,
            );
            self.elements.push(created);
        }
        let index = self.elements.len() - 2;
        (index, index + 1)
    }

    fn blur_element(
        &self,
        encoder: &mut CommandEncoder,
        source: &TextureView,
        a: usize,
        b: usize,
        rect: [f32; 4],
        element: usize,
    ) {
        let width = rect[2].max(1.0) as u32;
        let height = rect[3].max(1.0) as u32;
        for pass in 0..BLUR_PASSES {
            let previous = if pass == 0 {
                source
            } else {
                &self.elements[if pass.is_multiple_of(2) { b } else { a }].view
            };
            let destination = if pass.is_multiple_of(2) { a } else { b };
            let offset = ((element * BLUR_PASSES + pass) as u64 * BLUR_STRIDE) as u32;
            let group = self.device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.ui.glass.group"),
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
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.blur_uniforms,
                            offset: 0,
                            size: std::num::NonZeroU64::new(size_of::<BlurUniforms>() as u64),
                        }),
                    },
                ],
            });
            let mut render = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.ui.glass.blur"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.elements[destination].view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: PassOperations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            render.set_bind_group(0, &group, &[offset]);
            render.set_pipeline(&self.box_pass);
            render.draw(0..3, 0..1);
            let _ = (width, height);
        }
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
        let buffer = instance_buffer(&self.device, "grana.ui.instances.figure", instances.len());
        self.queue
            .write_buffer(&buffer, 0, bytemuck::cast_slice(instances));
        let uniforms = Uniforms {
            viewport: [size[0] as f32, size[1] as f32],
            scale,
            time: self.time,
            pointer: [-1.0e6, -1.0e6],
            padding: [0.0; 2],
        };
        let uniform_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.figure.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let group = self.bind_group(
            atlas,
            &scene.view,
            &scene.view,
            &scene.view,
            &buffer,
            &uniform_buffer,
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

    fn bind_group(
        &self,
        atlas: &Atlas,
        backdrop: &TextureView,
        scene: &TextureView,
        sharp: &TextureView,
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
                BindGroupEntry {
                    binding: 8,
                    resource: BindingResource::TextureView(&self.map),
                },
                BindGroupEntry {
                    binding: 9,
                    resource: BindingResource::Sampler(&self.map_sampler),
                },
                BindGroupEntry {
                    binding: 10,
                    resource: BindingResource::TextureView(sharp),
                },
            ],
        })
    }
}

fn glass_rects(instances: &[Instance], scale: f32, size: [u32; 2]) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    for instance in instances {
        if instance.meta[0] != KIND_GLASS {
            continue;
        }
        let rect = instance.rect;
        let margin = GLASS_MARGIN * scale;
        let x0 = (rect[0] * scale - margin)
            .floor()
            .clamp(0.0, size[0] as f32);
        let y0 = (rect[1] * scale - margin)
            .floor()
            .clamp(0.0, size[1] as f32);
        let x1 = ((rect[0] + rect[2]) * scale + margin)
            .ceil()
            .clamp(0.0, size[0] as f32);
        let y1 = ((rect[1] + rect[3]) * scale + margin)
            .ceil()
            .clamp(0.0, size[1] as f32);
        rects.push([x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0)]);
    }
    rects
}

fn segments(instances: &[Instance]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    for (index, instance) in instances.iter().enumerate() {
        if instance.meta[0] != KIND_GLASS {
            continue;
        }
        if index > start {
            segments.push(Segment::Batch {
                start,
                len: index - start,
            });
        }
        segments.push(Segment::Glass { index });
        start = index + 1;
    }
    if start < instances.len() {
        segments.push(Segment::Batch {
            start,
            len: instances.len() - start,
        });
    }
    segments
}

fn element_target(
    device: &Device,
    label: &str,
    size: [u32; 2],
    format: TextureFormat,
) -> ElementTarget {
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
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    ElementTarget {
        _texture: texture,
        view,
        size,
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

fn create_target(device: &Device, label: &str, size: [u32; 2], format: TextureFormat) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn target(device: &Device, label: &str, size: [u32; 2], format: TextureFormat) -> Target {
    let texture = create_target(device, label, size, format);
    let view = texture.create_view(&TextureViewDescriptor::default());
    Target { texture, view }
}

fn clear_target(encoder: &mut CommandEncoder, view: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.ui.clear.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: PassOperations {
                load: LoadOp::Clear(wgpu::Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

fn upload_map(device: &Device, queue: &Queue) -> TextureView {
    let decoder = png::Decoder::new(std::io::Cursor::new(MAP_BYTES));
    let mut reader = decoder.read_info().expect("the displacement map decodes");
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .expect("the map buffer size is known")
    ];
    let info = reader
        .next_frame(&mut buffer)
        .expect("the displacement map holds one frame");
    assert_eq!(
        (info.width, info.height),
        (256, 256),
        "the displacement map is square"
    );
    let rgba = match info.color_type {
        png::ColorType::Rgb => buffer[..info.buffer_size()]
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect::<Vec<u8>>(),
        png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
        other => panic!("unexpected displacement map color type: {other:?}"),
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.ui.map"),
        size: Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * 4),
            rows_per_image: Some(256),
        },
        Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&TextureViewDescriptor::default())
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
