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
use crate::temporal::SAMPLE_CEILING;

const SHADER: &str = include_str!("../shader/convergence.wgsl");

const WORKGROUP: u32 = 8;

const WORDS: u64 = 2 * size_of::<u32>() as u64;

const SLOTS: usize = 2;

const ERROR_LIMIT: f32 = 0.025;

pub(crate) struct Convergence {
    pipeline: ComputePipeline,
    layout: BindGroupLayout,
    limit: Buffer,
    worst: Buffer,
    readback: Readback,
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
            size: 4 * size_of::<u32>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let worst = device.create_buffer(&BufferDescriptor {
            label: Some("grana.temporal.convergence.worst"),
            size: WORDS,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = Readback::new(device, "grana.temporal.convergence.readback", WORDS, SLOTS);
        Self {
            pipeline,
            layout,
            limit,
            worst,
            readback,
            settled: false,
        }
    }

    pub(crate) fn converged(&self) -> bool {
        self.settled
    }

    pub(crate) fn invalidate(&mut self) {
        self.settled = false;
    }

    pub(crate) fn poll(&mut self, device: &Device) -> Result<(), String> {
        let measured = self.readback.take_with(device, |_, bytes| {
            let error = u32::from_ne_bytes(
                bytes[..4]
                    .try_into()
                    .expect("a convergence readback holds a whole word"),
            );
            let count = u32::from_ne_bytes(
                bytes[4..]
                    .try_into()
                    .expect("a convergence readback holds a whole word"),
            );
            (f32::from_bits(error), f32::from_bits(count))
        })?;
        if let Some((error, count)) = measured {
            self.settled = error <= ERROR_LIMIT || count >= SAMPLE_CEILING;
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
    ) {
        assert!(
            region[2] > 0 && region[3] > 0,
            "a convergence reduction covers a positive surface"
        );
        let Some(slot) = self.readback.reserve() else {
            return;
        };
        queue.write_buffer(&self.limit, 0, bytemuck::bytes_of(&region));
        queue.write_buffer(&self.worst, 0, bytemuck::bytes_of(&[0u32, u32::MAX]));
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
                    resource: BindingResource::TextureView(history.write_statistics()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(history.resolved()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: self.worst.as_entire_binding(),
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
        self.readback.stage(encoder, slot, 0, &self.worst, WORDS);
    }

    pub(crate) fn register(&mut self) {
        self.readback.register();
    }
}
