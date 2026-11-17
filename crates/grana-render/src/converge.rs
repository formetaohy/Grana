use std::collections::VecDeque;

use grana_gpu::Pipelines;
use grana_gpu::Readback;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePipeline, ComputePipelineDescriptor, Device, PipelineLayoutDescriptor,
    Queue, ShaderModuleDescriptor, ShaderSource, ShaderStages, TextureSampleType, TextureView,
    TextureViewDimension,
};

const MEASURE: &str = include_str!("shader/converge_measure.wgsl");
const REDUCE: &str = include_str!("shader/converge_reduce.wgsl");
const ROW: u64 = 8;
const SLOTS: usize = 6;
const GROUP: u32 = 256;
const GROUPS: u32 = 1024;
const FIRST: u32 = 2;
const GROWTH_NUMERATOR: u32 = 4;
const GROWTH_DENOMINATOR: u32 = 3;
const DEVIATION_TO_SIGMA: f32 = 1.2533;
const WINDOW: usize = 4;
const REPORT_FLOOR: usize = 3;
const WEIGHT_FLOOR: f32 = 1.0e-6;

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
    observations: VecDeque<Observation>,
    generation: u64,
    measured: u32,
}

#[derive(Clone, Copy)]
struct Observation {
    samples: f32,
    deviation: f32,
}

struct Stages {
    measure: ComputePipeline,
    reduce: ComputePipeline,
    measure_layout: BindGroupLayout,
    reduce_layout: BindGroupLayout,
}

struct Target {
    region: [u32; 2],
    groups: u32,
    frame: Buffer,
    counts: Buffer,
    total: Buffer,
    measure: BindGroup,
    reduce: BindGroup,
}

pub(crate) struct Sampled<'a> {
    pub(crate) region: [u32; 4],
    pub(crate) color: &'a TextureView,
    pub(crate) accumulation: &'a TextureView,
}

impl Convergence {
    pub(crate) fn new(device: &Device, pipelines: &Pipelines) -> Self {
        let measure_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.converge.measure.layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                uniform_entry(2),
                storage_entry(3, false),
            ],
        });
        let reduce_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.converge.reduce.layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                uniform_entry(2),
            ],
        });
        let measure = pipeline(
            device,
            "grana.converge.measure",
            MEASURE,
            &measure_layout,
            "measure",
            pipelines,
        );
        let reduce = pipeline(
            device,
            "grana.converge.reduce",
            REDUCE,
            &reduce_layout,
            "reduce",
            pipelines,
        );
        Self {
            pipelines: Stages {
                measure,
                reduce,
                measure_layout,
                reduce_layout,
            },
            target: None,
            readback: Readback::new(device, "grana.converge", ROW, SLOTS),
            queued: VecDeque::new(),
            observations: VecDeque::new(),
            generation: 0,
            measured: 0,
        }
    }

    pub(crate) fn forget(&mut self) {
        self.target = None;
        self.observations.clear();
        self.measured = 0;
        self.generation += 1;
    }

    pub(crate) fn reset(&mut self) {
        self.observations.clear();
        self.measured = 0;
        self.generation += 1;
    }

    fn due(&self, samples: u32) -> bool {
        samples >= FIRST
            && samples
                >= self
                    .measured
                    .saturating_mul(GROWTH_NUMERATOR)
                    .div_ceil(GROWTH_DENOMINATOR)
    }

    pub(crate) fn encode(
        &mut self,
        device: &Device,
        queue: &Queue,
        paint: &mut CommandEncoder,
        sampled: Sampled<'_>,
        samples: u32,
    ) {
        if !self.due(samples) {
            return;
        }
        let Sampled {
            region,
            color,
            accumulation,
        } = sampled;
        let Some(slot) = self.readback.reserve() else {
            return;
        };
        let size = [region[2].max(1), region[3].max(1)];
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.region != size)
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
        self.measured = samples;
        let generation = self.generation;
        let target = self.target.as_ref().expect("the converge target is ready");
        queue.write_buffer(
            &target.frame,
            0,
            bytemuck::bytes_of(&[region[0], region[1], size[0], size[1]]),
        );
        queue.write_buffer(
            &target.counts,
            0,
            bytemuck::bytes_of(&[target.groups, 0u32, 0u32, 0u32]),
        );
        {
            let mut pass = paint.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.converge.measure.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.measure);
            pass.set_bind_group(0, &target.measure, &[]);
            pass.dispatch_workgroups(target.groups, 1, 1);
        }
        {
            let mut pass = paint.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.converge.reduce.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.reduce);
            pass.set_bind_group(0, &target.reduce, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        self.readback.stage(paint, slot, 0, &target.total, ROW);
        self.queued.push_back((generation, samples));
    }

    pub(crate) fn register(&mut self) {
        self.readback.register();
    }

    pub(crate) fn poll(&mut self, device: &Device, drawn: u32) -> Option<Estimate> {
        let mut fresh = None;
        while let Some((deviation, weight)) = self
            .readback
            .take_with(device, |_, bytes| (sample(bytes, 0), sample(bytes, 1)))
        {
            let Some((generation, samples)) = self.queued.pop_front() else {
                continue;
            };
            if generation != self.generation {
                continue;
            }
            if weight <= WEIGHT_FLOOR {
                continue;
            }
            self.observations.push_back(Observation {
                samples: samples as f32,
                deviation: deviation / weight,
            });
            if self.observations.len() > WINDOW {
                self.observations.pop_front();
            }
            if self.observations.len() < REPORT_FLOOR {
                continue;
            }
            let slope = slope(&self.observations);
            let predicted = drawn.max(samples).max(1);
            fresh = Some(Estimate {
                samples: predicted,
                error: slope / (predicted as f32).sqrt(),
            });
        }
        fresh
    }
}

fn slope(observations: &VecDeque<Observation>) -> f32 {
    let mut weighted = 0.0;
    let mut reach = 0.0;
    for observation in observations {
        weighted += DEVIATION_TO_SIGMA * observation.deviation / observation.samples;
        reach += 1.0 / observation.samples;
    }
    weighted / reach
}

fn sample(bytes: &[u8], index: usize) -> f32 {
    let offset = index * size_of::<f32>();
    f32::from_le_bytes(
        bytes[offset..offset + size_of::<f32>()]
            .try_into()
            .expect("a deviation sample holds four bytes"),
    )
}

impl Target {
    fn new(
        device: &Device,
        stages: &Stages,
        size: [u32; 2],
        color: &TextureView,
        accumulation: &TextureView,
    ) -> Self {
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let groups = pixels
            .div_ceil(u64::from(GROUP))
            .clamp(1, u64::from(GROUPS)) as u32;
        let frame = device.create_buffer(&BufferDescriptor {
            label: Some("grana.converge.frame"),
            size: 4 * size_of::<u32>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let counts = device.create_buffer(&BufferDescriptor {
            label: Some("grana.converge.counts"),
            size: 4 * size_of::<u32>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let partials = device.create_buffer(&BufferDescriptor {
            label: Some("grana.converge.partials"),
            size: u64::from(GROUPS) * 2 * size_of::<f32>() as u64,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let total = device.create_buffer(&BufferDescriptor {
            label: Some("grana.converge.total"),
            size: 2 * size_of::<f32>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let measure = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.converge.measure.group"),
            layout: &stages.measure_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(color),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(accumulation),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: partials.as_entire_binding(),
                },
            ],
        });
        let reduce = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.converge.reduce.group"),
            layout: &stages.reduce_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: partials.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: total.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: counts.as_entire_binding(),
                },
            ],
        });
        Self {
            region: size,
            groups,
            frame,
            counts,
            total,
            measure,
            reduce,
        }
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn pipeline(
    device: &Device,
    label: &'static str,
    source: &str,
    layout: &BindGroupLayout,
    entry: &str,
    pipelines: &Pipelines,
) -> ComputePipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: pipelines.cache(),
    })
}
