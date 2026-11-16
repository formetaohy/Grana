use std::collections::VecDeque;

use grana_gpu::Pipelines;
use grana_gpu::Readback;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, ColorTargetState, ColorWrites,
    CommandEncoder, Device, FragmentState, LoadOp, Operations, PipelineLayoutDescriptor,
    PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

const SHADER: &str = include_str!("shader/converge.wgsl");
const FORMAT: TextureFormat = TextureFormat::Rg16Float;
const ROW: u64 = 256;
const SLOTS: usize = 6;
const DEVIATION_TO_SIGMA: f32 = 1.2533;
const WINDOW: usize = 4;
const REPORT_FLOOR: usize = 2;
const WEIGHT_FLOOR: f32 = 1.0e-6;
const LEVEL_LIMIT: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampling {
    floor: u32,
    ceiling: u32,
    error: f32,
}

impl Sampling {
    pub const fn new(floor: u32, ceiling: u32, error: f32) -> Self {
        assert!(floor > 0, "a sampling holds at least one sample");
        assert!(
            ceiling >= floor,
            "a sampling ceiling covers its floor of samples"
        );
        assert!(
            error > 0.0,
            "a sampling resolves below a positive relative error"
        );
        Self {
            floor,
            ceiling,
            error,
        }
    }

    pub const fn single() -> Self {
        Self {
            floor: 1,
            ceiling: 1,
            error: 1.0,
        }
    }

    pub fn floor(self) -> u32 {
        self.floor
    }

    pub fn ceiling(self) -> u32 {
        self.ceiling
    }

    pub fn error(self) -> f32 {
        self.error
    }

    pub fn measuring(self) -> bool {
        self.ceiling > 1 && self.error < f32::INFINITY
    }

    pub fn met(self, estimate: Option<Estimate>) -> bool {
        estimate
            .is_some_and(|estimate| estimate.samples >= self.floor && estimate.error <= self.error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    samples: u32,
    error: f32,
}

impl Estimate {
    pub fn samples(self) -> u32 {
        self.samples
    }

    pub fn error(self) -> f32 {
        self.error
    }
}

pub(crate) struct Convergence {
    pipelines: Stages,
    target: Option<Target>,
    readback: Readback,
    queued: VecDeque<(u64, u32)>,
    deviations: VecDeque<f32>,
    generation: u64,
}

struct Stages {
    error: RenderPipeline,
    reduce: RenderPipeline,
    error_layout: BindGroupLayout,
    reduce_layout: BindGroupLayout,
}

struct Target {
    size: [u32; 2],
    texture: Texture,
    views: Vec<TextureView>,
    error: BindGroup,
    reduces: Vec<BindGroup>,
}

impl Convergence {
    pub(crate) fn new(device: &Device, pipelines: &Pipelines) -> Self {
        let error_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.converge.error.layout"),
            entries: &[texture_entry(0), texture_entry(1)],
        });
        let reduce_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.converge.reduce.layout"),
            entries: &[texture_entry(0)],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.converge"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let error = pipeline(
            device,
            "grana.converge.error",
            &shader,
            &error_layout,
            "fs_error",
            pipelines,
        );
        let reduce = pipeline(
            device,
            "grana.converge.reduce",
            &shader,
            &reduce_layout,
            "fs_reduce",
            pipelines,
        );
        Self {
            pipelines: Stages {
                error,
                reduce,
                error_layout,
                reduce_layout,
            },
            target: None,
            readback: Readback::new(device, "grana.converge", ROW, SLOTS),
            queued: VecDeque::new(),
            deviations: VecDeque::new(),
            generation: 0,
        }
    }

    pub(crate) fn forget(&mut self) {
        self.target = None;
        self.deviations.clear();
        self.generation += 1;
    }

    pub(crate) fn reset(&mut self) {
        self.deviations.clear();
        self.generation += 1;
    }

    pub(crate) fn encode(
        &mut self,
        device: &Device,
        paint: &mut CommandEncoder,
        size: [u32; 2],
        color: &TextureView,
        accumulation: &TextureView,
        samples: u32,
    ) {
        let Some(slot) = self.readback.reserve() else {
            return;
        };
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            self.target = Some(Target::new(
                device,
                &self.pipelines,
                size,
                color,
                accumulation,
            ));
            self.generation += 1;
        }
        let generation = self.generation;
        let target = self.target.as_ref().expect("the converge target is ready");
        let mut pass = paint.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.converge.error.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &target.views[0],
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipelines.error);
        pass.set_bind_group(0, &target.error, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        for level in 1..target.views.len() {
            let mut pass = paint.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.converge.reduce.pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &target.views[level],
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines.reduce);
            pass.set_bind_group(0, &target.reduces[level - 1], &[]);
            pass.draw(0..3, 0..1);
        }
        let level = (target.views.len() - 1) as u32;
        self.readback
            .stage_level(paint, slot, &target.texture, level, ROW);
        self.queued.push_back((generation, samples));
    }

    pub(crate) fn register(&mut self) {
        self.readback.register();
    }

    pub(crate) fn poll(&mut self, device: &Device) -> Option<Estimate> {
        let mut fresh = None;
        while let Some((_, bytes)) = self.readback.take(device) {
            let Some((generation, samples)) = self.queued.pop_front() else {
                continue;
            };
            if generation != self.generation {
                continue;
            }
            let weight = sample(&bytes, 1);
            if weight <= WEIGHT_FLOOR {
                continue;
            }
            let deviation = sample(&bytes, 0) / weight;
            self.deviations.push_back(deviation);
            if self.deviations.len() > WINDOW {
                self.deviations.pop_front();
            }
            if self.deviations.len() >= REPORT_FLOOR {
                let smoothed = self.deviations.iter().sum::<f32>() / self.deviations.len() as f32;
                fresh = Some(Estimate {
                    samples,
                    error: DEVIATION_TO_SIGMA * smoothed / (samples as f32).sqrt(),
                });
            }
        }
        fresh
    }
}

fn sample(bytes: &[u8], index: usize) -> f32 {
    let offset = index * 2;
    half::f16::from_bits(u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("a deviation sample holds two bytes"),
    ))
    .to_f32()
}

impl Target {
    fn new(
        device: &Device,
        stages: &Stages,
        size: [u32; 2],
        color: &TextureView,
        accumulation: &TextureView,
    ) -> Self {
        let levels = u32::BITS - size[0].max(size[1]).leading_zeros();
        let levels = levels.clamp(1, LEVEL_LIMIT);
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.converge"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views = (0..levels)
            .map(|level| {
                texture.create_view(&TextureViewDescriptor {
                    label: Some("grana.converge.level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        let error = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.converge.error.group"),
            layout: &stages.error_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(color),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(accumulation),
                },
            ],
        });
        let reduces = (1..levels as usize)
            .map(|level| {
                device.create_bind_group(&BindGroupDescriptor {
                    label: Some("grana.converge.reduce.group"),
                    layout: &stages.reduce_layout,
                    entries: &[BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&views[level - 1]),
                    }],
                })
            })
            .collect();
        Self {
            size,
            texture,
            views,
            error,
            reduces,
        }
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn pipeline(
    device: &Device,
    label: &'static str,
    shader: &wgpu::ShaderModule,
    layout: &BindGroupLayout,
    fragment: &str,
    pipelines: &Pipelines,
) -> RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
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
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: pipelines.cache(),
    })
}
