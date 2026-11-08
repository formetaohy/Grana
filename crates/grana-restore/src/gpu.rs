use std::collections::HashMap;

use grana_optics::KernelBank;

use crate::Job;

const WORKGROUP: [u32; 3] = [8, 8, 4];
const BATCH_VOXELS: u64 = 16 << 20;
const POOL_BYTES: u64 = 192 << 20;
const READ: wgpu::BufferBindingType = wgpu::BufferBindingType::Storage { read_only: true };
const WRITE: wgpu::BufferBindingType = wgpu::BufferBindingType::Storage { read_only: false };

struct Pipelines {
    layout: wgpu::BindGroupLayout,
    blur: wgpu::ComputePipeline,
    correct: wgpu::ComputePipeline,
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grana.restore"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader/restore.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grana.restore.layout"),
            entries: &[
                binding(0, wgpu::BufferBindingType::Uniform),
                binding(1, READ),
                binding(2, READ),
                binding(3, READ),
                binding(4, WRITE),
                binding(5, WRITE),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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
            blur: pipeline("grana.restore.blur", "blur"),
            correct: pipeline("grana.restore.correct", "correct"),
        }
    }
}

fn binding(index: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: index,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Dims {
    volume: [u32; 4],
    kernel: [u32; 4],
    bank: [u32; 4],
}

struct Workspace {
    dims: [u32; 3],
    voxels: u64,
    taps_bytes: u64,
    uniforms: wgpu::Buffer,
    taps: wgpu::Buffer,
    measured: wgpu::Buffer,
    estimate: [wgpu::Buffer; 2],
    blurred: wgpu::Buffer,
    staging: wgpu::Buffer,
    groups: [wgpu::BindGroup; 2],
}

impl Workspace {
    fn bytes(&self) -> u64 {
        self.voxels * 4
    }

    fn memory(&self) -> u64 {
        self.measured.size()
            + self.estimate[0].size()
            + self.estimate[1].size()
            + self.blurred.size()
            + self.staging.size()
            + self.taps.size()
            + self.uniforms.size()
    }
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    workspaces: HashMap<[u32; 3], Vec<Workspace>>,
    pooled: u64,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        let headless = grana_gpu::Headless::new()?;
        let pipelines = Pipelines::new(&headless.device);
        Ok(Self {
            device: headless.device,
            queue: headless.queue,
            pipelines,
            workspaces: HashMap::new(),
            pooled: 0,
        })
    }

    pub fn restore_batch(&mut self, bank: &KernelBank, jobs: &[Job<'_>]) -> Vec<Vec<f32>> {
        let taps_bytes = bank.values().len() as u64 * 4;
        let mut results = Vec::with_capacity(jobs.len());
        let mut start = 0;
        while start < jobs.len() {
            let mut end = start;
            let mut voxels = 0;
            while end < jobs.len() {
                let voxels_of = voxels_of(jobs[end].dims);
                if end > start && voxels + voxels_of > BATCH_VOXELS {
                    break;
                }
                voxels += voxels_of;
                end += 1;
            }
            results.extend(self.run(bank, taps_bytes, &jobs[start..end]));
            start = end;
        }
        results
    }

    fn run(&mut self, bank: &KernelBank, taps_bytes: u64, jobs: &[Job<'_>]) -> Vec<Vec<f32>> {
        let kernel = bank.dims();
        let mut active = Vec::with_capacity(jobs.len());
        for job in jobs {
            let mut workspace = self.take(job.dims, taps_bytes);
            let uniform = Dims {
                volume: [job.dims[0], job.dims[1], job.dims[2], 0],
                kernel: [kernel[0], kernel[1], kernel[2], 0],
                bank: [bank.layers(), bank.band(), job.first_plane, 0],
            };
            self.queue
                .write_buffer(&workspace.uniforms, 0, bytemuck::bytes_of(&uniform));
            self.queue
                .write_buffer(&workspace.taps, 0, bytemuck::cast_slice(bank.values()));
            let mut values = job.source.to_vec();
            for value in &mut values {
                if !value.is_finite() || *value < 0.0 {
                    *value = 0.0;
                }
            }
            let bytes = bytemuck::cast_slice(&values);
            self.queue.write_buffer(&workspace.measured, 0, bytes);
            for buffer in &workspace.estimate {
                self.queue.write_buffer(buffer, 0, bytes);
            }
            workspace.dims = job.dims;
            active.push((workspace, job.iterations));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.restore"),
            });
        for (workspace, iterations) in &active {
            let dispatch = [
                workspace.dims[0].div_ceil(WORKGROUP[0]),
                workspace.dims[1].div_ceil(WORKGROUP[1]),
                workspace.dims[2].div_ceil(WORKGROUP[2]),
            ];
            for iteration in 0..*iterations {
                let group = &workspace.groups[(iteration % 2) as usize];
                for pipeline in [&self.pipelines.blur, &self.pipelines.correct] {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("grana.restore.pass"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, group, &[]);
                    pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
                }
            }
            let bytes = workspace.bytes();
            let source = &workspace.estimate[(*iterations % 2) as usize];
            encoder.copy_buffer_to_buffer(source, 0, &workspace.staging, 0, bytes);
        }
        self.queue.submit([encoder.finish()]);
        let mut mapped = Vec::with_capacity(active.len());
        for (workspace, _) in &active {
            let slice = workspace.staging.slice(..workspace.bytes());
            let (sender, receiver) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
            mapped.push((slice, receiver));
        }
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the graphics device failed while restoring");
        let mut results = Vec::with_capacity(active.len());
        for (index, (slice, receiver)) in mapped.into_iter().enumerate() {
            receiver
                .recv()
                .expect("the restore mapping callback runs once")
                .expect("the restore staging buffer is mappable");
            let range = slice
                .get_mapped_range()
                .expect("the mapped restore buffer stays mapped");
            results.push(bytemuck::cast_slice::<u8, f32>(&range).to_vec());
            drop(range);
            active[index].0.staging.unmap();
        }
        for (workspace, _) in active {
            self.store(workspace);
        }
        results
    }

    fn take(&mut self, dims: [u32; 3], taps_bytes: u64) -> Workspace {
        if let Some(pool) = self.workspaces.get_mut(&dims) {
            while let Some(workspace) = pool.pop() {
                if workspace.taps_bytes >= taps_bytes {
                    return workspace;
                }
                self.pooled -= workspace.memory();
            }
        }
        self.create(dims, taps_bytes)
    }

    fn store(&mut self, workspace: Workspace) {
        let memory = workspace.memory();
        if self.pooled + memory > POOL_BYTES {
            return;
        }
        self.pooled += memory;
        self.workspaces
            .entry(workspace.dims)
            .or_default()
            .push(workspace);
    }

    fn create(&self, dims: [u32; 3], taps_bytes: u64) -> Workspace {
        let device = &self.device;
        let voxels = voxels_of(dims);
        let volume = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: voxels * 4,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.uniforms"),
            size: size_of::<Dims>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let taps = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.taps"),
            size: taps_bytes.max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.staging"),
            size: voxels * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let measured = volume("grana.restore.measured");
        let estimate = [
            volume("grana.restore.estimate"),
            volume("grana.restore.estimate"),
        ];
        let blurred = volume("grana.restore.blurred");
        let groups = [0usize, 1].map(|swap| {
            let entries = [
                uniforms.as_entire_binding(),
                taps.as_entire_binding(),
                measured.as_entire_binding(),
                estimate[swap].as_entire_binding(),
                blurred.as_entire_binding(),
                estimate[1 - swap].as_entire_binding(),
            ];
            let entries = entries
                .into_iter()
                .enumerate()
                .map(|(index, resource)| wgpu::BindGroupEntry {
                    binding: index as u32,
                    resource,
                })
                .collect::<Vec<_>>();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("grana.restore.group"),
                layout: &self.pipelines.layout,
                entries: &entries,
            })
        });
        Workspace {
            dims,
            voxels,
            taps_bytes,
            uniforms,
            taps,
            measured,
            estimate,
            blurred,
            staging,
            groups,
        }
    }
}

fn voxels_of(dims: [u32; 3]) -> u64 {
    u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2])
}
