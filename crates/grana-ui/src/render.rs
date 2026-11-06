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
use crate::glass::{HUD, MENU, OVERLAY};
use crate::paint::{Instance, LayerInstances};
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

const PLUS_LIGHTER: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

const MULTIPLY: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::Dst,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ink {
    Light,
    Dark,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Front,
    Back,
}

impl Slot {
    fn other(self) -> Self {
        match self {
            Self::Front => Self::Back,
            Self::Back => Self::Front,
        }
    }
}

pub(crate) struct PaintFrame<'a> {
    pub target: &'a TextureView,
    pub atlas: &'a Atlas,
    pub content: &'a [Instance],
    pub chrome: &'a LayerInstances,
    pub labels: &'a LayerInstances,
    pub popup: &'a LayerInstances,
    pub hud: &'a LayerInstances,
    pub glass: &'a GlassScene,
    pub glass_on_glass: bool,
    pub base_changed: bool,
    pub interface_changed: bool,
    pub ink: Ink,
}

struct PaintTarget<'a> {
    target: &'a TextureView,
    scene: &'a TextureView,
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

    fn upload(&mut self, device: &Device, queue: &Queue, label: &str, instances: &[Instance]) {
        upload_instances(
            device,
            queue,
            label,
            instances,
            &mut self.buffer,
            &mut self.capacity,
        );
    }
}

struct LayerBuffers {
    material: InstanceBuffer,
    ink: InstanceBuffer,
}

impl LayerBuffers {
    fn new(device: &Device, label: &str) -> Self {
        Self {
            material: InstanceBuffer::new(device, &format!("{label}.material")),
            ink: InstanceBuffer::new(device, &format!("{label}.ink")),
        }
    }

    fn upload(&mut self, device: &Device, queue: &Queue, label: &str, instances: &LayerInstances) {
        self.material.upload(
            device,
            queue,
            &format!("{label}.material"),
            &instances.material,
        );
        self.ink
            .upload(device, queue, &format!("{label}.ink"), &instances.ink);
    }
}

struct Buffers {
    content: InstanceBuffer,
    chrome: LayerBuffers,
    labels: LayerBuffers,
    popup: LayerBuffers,
    hud: LayerBuffers,
}

impl Buffers {
    fn new(device: &Device) -> Self {
        Self {
            content: InstanceBuffer::new(device, "grana.ui.instances.content"),
            chrome: LayerBuffers::new(device, "grana.ui.instances.chrome"),
            labels: LayerBuffers::new(device, "grana.ui.instances.labels"),
            popup: LayerBuffers::new(device, "grana.ui.instances.popup"),
            hud: LayerBuffers::new(device, "grana.ui.instances.hud"),
        }
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
    base: Option<Surface>,
    front: Option<Surface>,
    back: Option<Surface>,
    blit: RenderPipeline,
    present_encoded: RenderPipeline,
    present_linear: RenderPipeline,
    ui: RenderPipeline,
    ui_add: RenderPipeline,
    ui_multiply: RenderPipeline,
    figure: RenderPipeline,
    lower: GlassRenderer,
    overlay: GlassRenderer,
    menu_glass: GlassRenderer,
    hud_glass: GlassRenderer,
    ui_layout: BindGroupLayout,
    blit_layout: BindGroupLayout,
    sampler: Sampler,
    uniforms: Buffer,
    buffers: Buffers,
    figure_instances: Buffer,
    figure_capacity: usize,
    figure_uniforms: Buffer,
    presented: Option<Slot>,
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
            PREMULTIPLIED,
        );
        let ui_add = instance_pipeline(
            device,
            "grana.ui.instances.add",
            &ui_pipeline_layout,
            &ui_shader,
            raw(encoded),
            PLUS_LIGHTER,
        );
        let ui_multiply = instance_pipeline(
            device,
            "grana.ui.instances.multiply",
            &ui_pipeline_layout,
            &ui_shader,
            raw(encoded),
            MULTIPLY,
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
            scene: None,
            base: None,
            front: None,
            back: None,
            blit,
            present_encoded,
            present_linear,
            ui,
            ui_add,
            ui_multiply,
            figure,
            lower: GlassRenderer::new(device, encoded),
            overlay: GlassRenderer::new(device, encoded),
            menu_glass: GlassRenderer::new(device, encoded),
            hud_glass: GlassRenderer::new(device, encoded),
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
            presented: None,
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
        self.base = Some(Surface::new(&device, "grana.ui.base", size, self.encoded));
        self.front = Some(Surface::new(&device, "grana.ui.front", size, self.encoded));
        self.back = Some(Surface::new(&device, "grana.ui.back", size, self.encoded));
        self.presented = None;
    }

    pub fn scene_view(&self) -> &TextureView {
        &self.scene.as_ref().expect("the renderer is sized").view
    }

    pub fn read_color(&mut self) -> (Vec<u8>, [u32; 2], TextureFormat) {
        let slot = self.presented.unwrap_or(Slot::Front);
        let surface = match slot {
            Slot::Front => self.front.as_ref(),
            Slot::Back => self.back.as_ref(),
        }
        .expect("the renderer is sized");
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
            content,
            chrome,
            labels,
            popup,
            hud,
            glass,
            glass_on_glass,
            base_changed,
            interface_changed,
            ink,
        } = frame;
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        let scene = self
            .scene
            .as_ref()
            .expect("the renderer is sized")
            .view
            .clone();
        let base = self.base.as_ref().expect("the renderer is sized");
        let front = self.front.as_ref().expect("the renderer is sized");
        let back = self.back.as_ref().expect("the renderer is sized");
        let base_view = base.linear.clone();
        let base_encoded = base.encoded.clone();
        let front_encoded = front.encoded.clone();
        let front_linear = front.linear.clone();
        let back_encoded = back.encoded.clone();
        let back_linear = back.linear.clone();
        self.buffers.content.upload(
            &self.device,
            &self.queue,
            "grana.ui.instances.content",
            content,
        );
        self.buffers.chrome.upload(
            &self.device,
            &self.queue,
            "grana.ui.instances.chrome",
            chrome,
        );
        self.buffers.labels.upload(
            &self.device,
            &self.queue,
            "grana.ui.instances.labels",
            labels,
        );
        self.buffers
            .popup
            .upload(&self.device, &self.queue, "grana.ui.instances.popup", popup);
        self.buffers
            .hud
            .upload(&self.device, &self.queue, "grana.ui.instances.hud", hud);
        let scale = self.scale;
        let size = (self.size[0], self.size[1]);

        let scene_group = self.blit_group(&scene);
        fullscreen(
            encoder,
            &self.blit,
            &scene_group,
            &base_view,
            LoadOp::Clear(wgpu::Color::BLACK),
        );
        self.paint_material(
            encoder,
            atlas,
            &PaintTarget {
                target: &base_view,
                scene: &scene,
                load: LoadOp::Load,
            },
            &self.buffers.content.buffer,
            content.len(),
        );

        self.lower.render_layers(
            &self.device,
            &self.queue,
            encoder,
            &GlassFrame {
                content: &base_encoded,
                target: &front_encoded,
                size,
                scale,
                content_changed: base_changed,
                copy: true,
            },
            glass,
            ..OVERLAY,
        );
        self.paint_layer(
            encoder,
            atlas,
            &PaintTarget {
                target: &front_linear,
                scene: &scene,
                load: LoadOp::Load,
            },
            &self.buffers.chrome,
            chrome,
            ink,
        );

        let mut current = Slot::Front;
        if glass.has_layers(OVERLAY..MENU) {
            let content = slot_view(current, &front_encoded, &back_encoded);
            let target = slot_view(current.other(), &front_encoded, &back_encoded);
            self.overlay.render_layers(
                &self.device,
                &self.queue,
                encoder,
                &GlassFrame {
                    content,
                    target,
                    size,
                    scale,
                    content_changed: interface_changed,
                    copy: true,
                },
                glass,
                OVERLAY..MENU,
            );
            let linear = slot_view(current.other(), &front_linear, &back_linear);
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.labels,
                labels,
                ink,
            );
            current = current.other();
        } else {
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: &front_linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.labels,
                labels,
                ink,
            );
        }

        if glass.has_layers(MENU..HUD) {
            let target_slot = if glass_on_glass {
                current.other()
            } else {
                current
            };
            let content = if glass_on_glass {
                slot_view(current, &front_encoded, &back_encoded)
            } else {
                &base_encoded
            };
            let target = slot_view(target_slot, &front_encoded, &back_encoded);
            self.menu_glass.render_layers(
                &self.device,
                &self.queue,
                encoder,
                &GlassFrame {
                    content,
                    target,
                    size,
                    scale,
                    content_changed: if glass_on_glass {
                        interface_changed
                    } else {
                        base_changed
                    },
                    copy: glass_on_glass,
                },
                glass,
                MENU..HUD,
            );
            let linear = slot_view(target_slot, &front_linear, &back_linear);
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.popup,
                popup,
                ink,
            );
            current = target_slot;
        } else {
            let linear = slot_view(current, &front_linear, &back_linear);
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.popup,
                popup,
                ink,
            );
        }

        if glass.has_layers(HUD..) {
            let target_slot = if glass_on_glass {
                current.other()
            } else {
                current
            };
            let content = if glass_on_glass {
                slot_view(current, &front_encoded, &back_encoded)
            } else {
                &base_encoded
            };
            let target = slot_view(target_slot, &front_encoded, &back_encoded);
            self.hud_glass.render_layers(
                &self.device,
                &self.queue,
                encoder,
                &GlassFrame {
                    content,
                    target,
                    size,
                    scale,
                    content_changed: if glass_on_glass {
                        interface_changed
                    } else {
                        base_changed
                    },
                    copy: glass_on_glass,
                },
                glass,
                HUD..,
            );
            let linear = slot_view(target_slot, &front_linear, &back_linear);
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.hud,
                hud,
                ink,
            );
            current = target_slot;
        } else {
            let linear = slot_view(current, &front_linear, &back_linear);
            self.paint_layer(
                encoder,
                atlas,
                &PaintTarget {
                    target: linear,
                    scene: &scene,
                    load: LoadOp::Load,
                },
                &self.buffers.hud,
                hud,
                ink,
            );
        }

        let present = slot_view(current, &front_encoded, &back_encoded);
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
        self.presented = Some(current);
    }

    fn paint_material(
        &self,
        encoder: &mut CommandEncoder,
        atlas: &Atlas,
        target: &PaintTarget<'_>,
        buffer: &Buffer,
        count: usize,
    ) {
        if count == 0 {
            return;
        }
        let group = self.ui_group(atlas, target.scene, buffer, &self.uniforms);
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

    fn paint_layer(
        &self,
        encoder: &mut CommandEncoder,
        atlas: &Atlas,
        target: &PaintTarget<'_>,
        buffers: &LayerBuffers,
        instances: &LayerInstances,
        ink: Ink,
    ) {
        if instances.is_empty() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.layer.pass"),
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
        if !instances.material.is_empty() {
            let group = self.ui_group(
                atlas,
                target.scene,
                &buffers.material.buffer,
                &self.uniforms,
            );
            pass.set_pipeline(&self.ui);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..6, 0..instances.material.len() as u32);
        }
        if !instances.ink.is_empty() {
            let group = self.ui_group(atlas, target.scene, &buffers.ink.buffer, &self.uniforms);
            pass.set_pipeline(match ink {
                Ink::Light => &self.ui_add,
                Ink::Dark => &self.ui_multiply,
            });
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..6, 0..instances.ink.len() as u32);
        }
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

fn slot_view<'a>(slot: Slot, front: &'a TextureView, back: &'a TextureView) -> &'a TextureView {
    match slot {
        Slot::Front => front,
        Slot::Back => back,
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
