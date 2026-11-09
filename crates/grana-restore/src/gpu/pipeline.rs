use grana_gpu::layout::{storage, uniform};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, ComputePipeline, Device, PipelineLayoutDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages,
};

pub(crate) struct Pipelines {
    pub layout: BindGroupLayout,
    pub pad_estimate: ComputePipeline,
    pub pad_ratio: ComputePipeline,
    pub blur: ComputePipeline,
    pub correct: ComputePipeline,
}

impl Pipelines {
    pub fn new(device: &Device) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.restore"),
            source: ShaderSource::Wgsl(include_str!("../shader/restore.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.restore.layout"),
            entries: &[
                uniform(0, ShaderStages::COMPUTE),
                storage(1, ShaderStages::COMPUTE, true),
                storage(2, ShaderStages::COMPUTE, true),
                storage(3, ShaderStages::COMPUTE, true),
                storage(4, ShaderStages::COMPUTE, true),
                storage(5, ShaderStages::COMPUTE, false),
                storage(6, ShaderStages::COMPUTE, false),
                storage(7, ShaderStages::COMPUTE, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.restore.pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            layout,
            pad_estimate: pipeline("grana.restore.pad.estimate", "pad_estimate"),
            pad_ratio: pipeline("grana.restore.pad.ratio", "pad_ratio"),
            blur: pipeline("grana.restore.blur", "blur"),
            correct: pipeline("grana.restore.correct", "correct"),
        }
    }
}
