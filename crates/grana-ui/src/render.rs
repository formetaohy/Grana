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
use crate::paint::{Instance, LayerBuffer};
use grana_gpu::Pipelines;
use grana_gpu::layout;

const BLIT_SHADER: &str = include_str!("shader/blit.wgsl");
const UI_SHADER: &str = include_str!("shader/ui.wgsl");
const MINIMUM_INSTANCES: usize = 512;
const OVERLAY_SLOT: usize = Layer::ALL.len();
const SLOTS: usize = OVERLAY_SLOT + 1;
const DIRECT_COVERAGE: u64 = 2;
const READBACK_ALIGNMENT: u32 = 256;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Damage {
    None,
    Regions {
        rects: [[u32; 4]; crate::paint::DAMAGE_REGIONS],
        len: u8,
    },
    Full,
}

impl Damage {
    fn scissors(
        self,
        clip: impl Fn([u32; 4]) -> [u32; 4],
        full: [u32; 4],
    ) -> ([[u32; 4]; crate::paint::DAMAGE_REGIONS], usize) {
        let mut scissors = [[0u32; 4]; crate::paint::DAMAGE_REGIONS];
        match self {
            Self::None => (scissors, 0),
            Self::Full => {
                scissors[0] = full;
                (scissors, 1)
            }
            Self::Regions { rects, len } => {
                let mut count = 0;
                for rect in &rects[..usize::from(len)] {
                    let rect = clip(*rect);
                    if rect[2] == 0 || rect[3] == 0 {
                        continue;
                    }
                    scissors[count] = rect;
                    count += 1;
                }
                (scissors, count)
            }
        }
    }
}

pub(crate) struct Layers<'a> {
    pub content: &'a LayerBuffer,
    pub chrome: &'a LayerBuffer,
    pub popup: &'a LayerBuffer,
    pub hud: &'a LayerBuffer,
    pub overlay: &'a LayerBuffer,
}

impl<'a> Layers<'a> {
    fn all(&self) -> [&'a LayerBuffer; SLOTS] {
        [
            self.content,
            self.chrome,
            self.popup,
            self.hud,
            self.overlay,
        ]
    }
}

pub(crate) struct PaintFrame<'a> {
    pub target: &'a TextureView,
    pub scene: Option<&'a TextureView>,
    pub scene_changed: bool,
    pub reset: bool,
    pub damage: Damage,
    pub atlases: Atlases<'a>,
    pub layers: Layers<'a>,
}

struct InstanceBuffer {
    buffer: Buffer,
    capacity: usize,
    generation: u64,
}

impl InstanceBuffer {
    fn new(device: &Device, label: &str) -> Self {
        Self {
            buffer: instance_buffer(device, label, MINIMUM_INSTANCES),
            capacity: MINIMUM_INSTANCES,
            generation: 0,
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
            self.generation = self.generation.wrapping_add(1);
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
    size: [u32; 2],
    scale: f32,
    canvas: Option<Canvas>,
    canvas_stale: bool,
    dummy: TextureView,
    blit: RenderPipeline,
    erase: RenderPipeline,
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
    ui_groups: [Option<(u64, BindGroup)>; SLOTS],
}

struct Canvas {
    texture: Texture,
    view: TextureView,
}

impl Canvas {
    fn new(device: &Device, size: [u32; 2], format: TextureFormat) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.ui.canvas"),
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
        Self { texture, view }
    }
}

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        pipelines: &Pipelines,
    ) -> Self {
        assert!(
            !format.is_srgb(),
            "the interface paints display space bytes into a linear surface"
        );
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
            entries: &[texture_entry(0)],
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
            "fs_copy",
            format,
            pipelines,
        );
        let erase_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.ui.erase.pipeline_layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let erase = fullscreen_pipeline(
            device,
            "grana.ui.erase",
            &erase_layout,
            &blit_shader,
            "fs_clear",
            format,
            pipelines,
        );
        let ui = instance_pipeline(
            device,
            "grana.ui.instances",
            &ui_pipeline_layout,
            &ui_shader,
            format,
            PREMULTIPLIED,
            pipelines,
        );
        let figure = instance_pipeline(
            device,
            "grana.ui.figure",
            &ui_pipeline_layout,
            &ui_shader,
            format,
            PREMULTIPLIED,
            pipelines,
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
            size: [0, 0],
            scale: 1.0,
            canvas: None,
            canvas_stale: true,
            dummy: dummy(device),
            blit,
            erase,
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
            ui_groups: std::array::from_fn(|_| None),
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        if size == self.size {
            return false;
        }
        self.size = size;
        self.canvas = Some(Canvas::new(&self.device, size, self.format));
        self.canvas_stale = true;
        self.canvas_generation = self.canvas_generation.wrapping_add(1);
        self.blit_cache = None;
        self.ui_groups = std::array::from_fn(|_| None);
        true
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn device(&self) -> &Device {
        &self.device
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
            damage,
            atlases,
            layers,
        } = frame;
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        let layers = layers.all();
        let reupload = self.upload(&layers);
        let damage = if reset || (scene_changed || reupload) && matches!(damage, Damage::None) {
            Damage::Full
        } else {
            damage
        };
        let (scissors, count) = damage.scissors(|rect| self.clip(rect), self.region());
        self.prepare_groups(atlases, scene);
        let groups = self.layer_groups(&layers);
        if matches!(damage, Damage::Full) || covers_surface(&scissors[..count], self.region()) {
            self.paint_whole(encoder, target_view, &groups);
            self.canvas_stale = true;
            return;
        }
        let canvas = self.canvas.as_ref().expect("the renderer is sized");
        if self.canvas_stale {
            self.paint_whole(encoder, &canvas.view, &groups);
            self.canvas_stale = false;
        } else if count > 0 {
            self.paint_layers(
                encoder,
                &canvas.view,
                &[groups[Layer::Content.index()]],
                &scissors[..count],
                true,
            );
            self.paint_layers(
                encoder,
                &canvas.view,
                &groups[1..],
                &scissors[..count],
                false,
            );
        }
        if self
            .blit_cache
            .as_ref()
            .is_none_or(|(generation, _)| *generation != self.canvas_generation)
        {
            let group = self.blit_group(&canvas.view);
            self.blit_cache = Some((self.canvas_generation, group));
        }
        let group = &self
            .blit_cache
            .as_ref()
            .expect("the blit group is cached")
            .1;
        fullscreen(
            encoder,
            &self.blit,
            group,
            target_view,
            LoadOp::Clear(wgpu::Color::BLACK),
        );
    }

    pub(crate) fn capture(
        &mut self,
        encoder: &mut CommandEncoder,
        atlases: Atlases<'_>,
        layers: Layers<'_>,
        scene: Option<&TextureView>,
    ) -> Texture {
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "the UI renderer needs a sized target"
        );
        let layers = layers.all();
        self.upload(&layers);
        self.prepare_groups(atlases, scene);
        let image = Canvas::new(&self.device, self.size, self.format);
        let groups = self.layer_groups(&layers);
        self.paint_whole(encoder, &image.view, &groups);
        image.texture
    }

    pub fn read_color(
        &self,
        mut encoder: CommandEncoder,
        texture: &Texture,
    ) -> (Vec<u8>, [u32; 2], TextureFormat) {
        let [width, height] = self.size;
        let padded = (width * 4).div_ceil(READBACK_ALIGNMENT) * READBACK_ALIGNMENT;
        let buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("grana.ui.readback"),
            size: u64::from(padded * height),
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture,
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
        (pixels, [width, height], self.format)
    }

    fn upload(&mut self, layers: &[&LayerBuffer; SLOTS]) -> bool {
        let changed: [bool; SLOTS] =
            std::array::from_fn(|index| self.revisions[index] != layers[index].revision);
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
        changed.iter().any(|changed| *changed)
    }

    fn layer_groups<'a>(
        &'a self,
        layers: &[&LayerBuffer; SLOTS],
    ) -> [(&'a BindGroup, usize); SLOTS] {
        std::array::from_fn(|index| {
            let group = &self.ui_groups[index]
                .as_ref()
                .expect("the interface group is cached")
                .1;
            (group, layers[index].material.len())
        })
    }

    fn paint_whole(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        layers: &[(&BindGroup, usize)],
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.whole.pass"),
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
        pass.set_pipeline(&self.ui);
        for (group, count) in layers {
            if *count == 0 {
                continue;
            }
            pass.set_bind_group(0, *group, &[]);
            pass.draw(0..6, 0..*count as u32);
        }
    }

    fn prepare_groups(&mut self, atlases: Atlases<'_>, scene: Option<&TextureView>) {
        let scene_id = scene.map_or(0, |view| view as *const TextureView as usize);
        let scene = scene.unwrap_or(&self.dummy);
        for slot in 0..SLOTS {
            let key = group_key(scene_id, self.buffers.buffers[slot].generation, atlases);
            if self.ui_groups[slot]
                .as_ref()
                .is_some_and(|(held, _)| *held == key)
            {
                continue;
            }
            let group = self.ui_group(
                atlases,
                scene,
                &self.buffers.buffers[slot].buffer,
                &self.uniforms,
            );
            self.ui_groups[slot] = Some((key, group));
        }
    }

    fn region(&self) -> [u32; 4] {
        [0, 0, self.size[0], self.size[1]]
    }

    fn clip(&self, rect: [u32; 4]) -> [u32; 4] {
        let x = rect[0].min(self.size[0]);
        let y = rect[1].min(self.size[1]);
        let width = rect[2].min(self.size[0] - x);
        let height = rect[3].min(self.size[1] - y);
        [x, y, width, height]
    }

    fn paint_layers(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        layers: &[(&BindGroup, usize)],
        scissors: &[[u32; 4]],
        erase: bool,
    ) {
        if !erase && layers.iter().all(|(_, count)| *count == 0) {
            return;
        }
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.ui.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: PassOperations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        for scissor in scissors {
            if erase {
                pass.set_pipeline(&self.erase);
                pass.set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(&self.ui);
            pass.set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
            for (group, count) in layers {
                if *count == 0 {
                    continue;
                }
                pass.set_bind_group(0, *group, &[]);
                pass.draw(0..6, 0..*count as u32);
            }
        }
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
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(source),
            }],
        })
    }

    fn ui_group(
        &self,
        atlases: Atlases<'_>,
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
            ],
        })
    }
}

fn group_key(scene: usize, instances: u64, atlases: Atlases<'_>) -> u64 {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    for value in [
        scene as u64,
        instances,
        atlases.glyphs.view() as *const TextureView as usize as u64,
        atlases.images.generation(),
    ] {
        key = (key ^ value).wrapping_mul(0x0000_0100_0000_01b3);
    }
    key
}

fn covers_surface(scissors: &[[u32; 4]], region: [u32; 4]) -> bool {
    let area = u64::from(region[2]) * u64::from(region[3]);
    if area == 0 {
        return true;
    }
    let damaged = scissors
        .iter()
        .map(|rect| u64::from(rect[2]) * u64::from(rect[3]))
        .sum::<u64>();
    damaged.saturating_mul(DIRECT_COVERAGE) >= area
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
    pipelines: &Pipelines,
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
        cache: pipelines.cache(),
    })
}

fn instance_pipeline(
    device: &Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &ShaderModule,
    format: TextureFormat,
    blend: BlendState,
    pipelines: &Pipelines,
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
        cache: pipelines.cache(),
    })
}
