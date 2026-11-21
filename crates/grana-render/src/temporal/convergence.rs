use std::collections::VecDeque;

use grana_gpu::Pipelines;
use grana_gpu::Readback;
use grana_gpu::layout::{storage, texture, uniform};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePassDescriptor, ComputePipeline, Device, PipelineLayoutDescriptor,
    Queue, ShaderModuleDescriptor, ShaderSource, ShaderStages, TextureSampleType,
    TextureViewDimension,
};

use super::History;

const SHADER: &str = concat!(
    include_str!("../shader/color.wgsl"),
    "\n",
    include_str!("../shader/convergence.wgsl")
);

const WORKGROUP: u32 = 8;

const BINS: usize = 64;

const WORDS: u64 = (BINS * size_of::<u32>()) as u64;

const SLOTS: usize = 2;

const CEILING: f32 = 0.05;

const LIMIT: f32 = 0.25 / 255.0;

pub(crate) struct Convergence {
    pipeline: ComputePipeline,
    layout: BindGroupLayout,
    limit: Buffer,
    bins: Buffer,
    readback: Readback,
    staged: VecDeque<u64>,
    generation: u64,
    settled: bool,
}

impl Convergence {
    pub(crate) fn new(device: &Device, pipelines: &Pipelines) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.temporal.convergence.layout"),
            entries: &[
                uniform(0, ShaderStages::COMPUTE),
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                texture(2, ShaderStages::COMPUTE),
                storage(3, ShaderStages::COMPUTE, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.temporal.convergence.pipeline.layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.temporal.convergence"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("grana.temporal.convergence"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("reduce"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        let limit = device.create_buffer(&BufferDescriptor {
            label: Some("grana.temporal.convergence.limit"),
            size: 4 * size_of::<u32>() as u64 + 4 * size_of::<f32>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bins = device.create_buffer(&BufferDescriptor {
            label: Some("grana.temporal.convergence.bins"),
            size: WORDS,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = Readback::new(device, "grana.temporal.convergence.readback", WORDS, SLOTS);
        Self {
            pipeline,
            layout,
            limit,
            bins,
            readback,
            staged: VecDeque::new(),
            generation: 0,
            settled: false,
        }
    }

    pub(crate) fn converged(&self) -> bool {
        self.settled
    }

    pub(crate) fn invalidate(&mut self) {
        self.settled = false;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn poll(&mut self, device: &Device) -> Result<(), String> {
        let measured = self.readback.take_with(device, |_, bytes| counts(bytes))?;
        if let Some(counts) = measured {
            let staged = self
                .staged
                .pop_front()
                .expect("every convergence readback is staged with its generation");
            if staged == self.generation {
                self.settled = settled(&counts);
            }
        }
        Ok(())
    }

    pub(crate) fn evaluate(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        history: &History,
        region: [u32; 4],
        exposure: f32,
    ) {
        assert!(
            region[2] > 0 && region[3] > 0,
            "a convergence reduction covers a positive surface"
        );
        assert!(
            exposure.is_finite() && exposure > 0.0,
            "a convergence reduction weighs the presented radiance"
        );
        let Some(slot) = self.readback.reserve() else {
            return;
        };
        self.staged.push_back(self.generation);
        queue.write_buffer(
            &self.limit,
            0,
            bytemuck::bytes_of(&Limit::new(region, exposure)),
        );
        queue.write_buffer(&self.bins, 0, bytemuck::cast_slice(&[0u32; BINS]));
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.temporal.convergence.group"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.limit.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(history.resolved()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(history.read_color()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: self.bins.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("grana.temporal.convergence.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(
                region[2].div_ceil(WORKGROUP),
                region[3].div_ceil(WORKGROUP),
                1,
            );
        }
        self.readback.stage(encoder, slot, 0, &self.bins, WORDS);
    }

    pub(crate) fn register(&mut self) {
        self.readback.register();
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Limit {
    region: [u32; 4],
    exposure: [f32; 4],
}

impl Limit {
    fn new(region: [u32; 4], exposure: f32) -> Self {
        Self {
            region,
            exposure: [exposure, 0.0, 0.0, 0.0],
        }
    }
}

fn counts(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_ne_bytes(*word))
        .collect()
}

fn settled(counts: &[u32]) -> bool {
    assert_eq!(
        counts.len(),
        BINS,
        "a convergence histogram holds every bin"
    );
    let total = counts.iter().map(|count| u64::from(*count)).sum::<u64>();
    if total == 0 {
        return true;
    }
    let mean = counts
        .iter()
        .enumerate()
        .map(|(index, count)| {
            f64::from(*count) * (index as f64 + 0.5) * f64::from(CEILING) / BINS as f64
        })
        .sum::<f64>()
        / total as f64;
    mean <= f64::from(LIMIT)
}
