mod pipeline;
mod workspace;

use std::collections::HashMap;

use grana_optics::KernelBank;

use self::pipeline::Pipelines;
use self::workspace::Workspace;

use crate::Job;

const WORKGROUP: [u32; 3] = [8, 8, 4];
const BLOCK: u32 = 4;
const BATCH_VOXELS: u64 = 16 << 20;
const BATCH_BYTES: u64 = 512 << 20;
const POOL_BYTES: u64 = 192 << 20;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Dims {
    volume: [u32; 4],
    kernel: [u32; 4],
    padded: [u32; 4],
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
        let kernel = bank.dims();
        let mut results = Vec::with_capacity(jobs.len());
        let mut start = 0;
        while start < jobs.len() {
            let mut end = start;
            let mut voxels = 0;
            let mut memory = 0;
            while end < jobs.len() {
                let job_voxels = voxels_of(jobs[end].dims);
                let job_memory = workspace_memory(jobs[end].dims, kernel, bank);
                if end > start
                    && (voxels + job_voxels > BATCH_VOXELS || memory + job_memory > BATCH_BYTES)
                {
                    break;
                }
                voxels += job_voxels;
                memory += job_memory;
                end += 1;
            }
            results.extend(self.run(bank, &jobs[start..end]));
            start = end;
        }
        results
    }

    fn run(&mut self, bank: &KernelBank, jobs: &[Job<'_>]) -> Vec<Vec<f32>> {
        let kernel = bank.dims();
        let mut active = Vec::with_capacity(jobs.len());
        for job in jobs {
            let bases = bases(job, bank);
            let workspace = self.take(job.dims, kernel, bank, &bases);
            let uniform = Dims {
                volume: [job.dims[0], job.dims[1], job.dims[2], 0],
                kernel: [kernel[0], kernel[1], kernel[2], 0],
                padded: [
                    workspace.padded[0],
                    workspace.padded[1],
                    workspace.padded[2],
                    workspace.stride,
                ],
            };
            workspace.write_uniforms(&self.queue, &uniform);
            let mut values = job.source.to_vec();
            for value in &mut values {
                if !value.is_finite() || *value < 0.0 {
                    *value = 0.0;
                }
            }
            workspace.write_source(&self.queue, &values);
            active.push((workspace, job.iterations));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.restore"),
            });
        for (workspace, iterations) in &active {
            let pad = pad_dispatch(workspace.padded);
            let convolution = convolution_dispatch(workspace.dims);
            let mut current = 0;
            for _ in 0..*iterations {
                for (pipeline, groups) in [
                    (&self.pipelines.pad_estimate, pad),
                    (&self.pipelines.blur, convolution),
                    (&self.pipelines.pad_ratio, pad),
                    (&self.pipelines.correct, convolution),
                ] {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("grana.restore.pass"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, workspace.group(current), &[]);
                    pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
                }
                current = 1 - current;
            }
            let bytes = workspace.bytes();
            encoder.copy_buffer_to_buffer(
                &workspace.estimate[current],
                0,
                &workspace.staging,
                0,
                bytes,
            );
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

    fn take(
        &mut self,
        dims: [u32; 3],
        kernel: [u32; 3],
        bank: &KernelBank,
        bases: &[u32],
    ) -> Workspace {
        let taps_bytes = bank.values().len() as u64 * 4;
        if let Some(pool) = self.workspaces.get_mut(&dims) {
            while let Some(workspace) = pool.pop() {
                if workspace.kernel == kernel && workspace.fits_taps(taps_bytes) {
                    self.queue.write_buffer(
                        &workspace.taps,
                        0,
                        bytemuck::cast_slice(bank.values()),
                    );
                    self.queue
                        .write_buffer(&workspace.bases, 0, bytemuck::cast_slice(bases));
                    self.pooled -= workspace.memory();
                    return workspace;
                }
                self.pooled -= workspace.memory();
            }
        }
        Workspace::new(
            &self.device,
            &self.queue,
            &self.pipelines,
            dims,
            kernel,
            bases,
            bank.values(),
        )
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
}

fn bases(job: &Job<'_>, bank: &KernelBank) -> Vec<u32> {
    let kernel = bank.dims();
    let voxels = kernel[0] * kernel[1] * kernel[2];
    let depth = job.dims[2] + kernel[2] - 1;
    (0..depth)
        .map(|padded| {
            let source = mirror(padded as i32 - (kernel[2] / 2) as i32, job.dims[2]);
            bank.layer(job.first_plane, source) * voxels
        })
        .collect()
}

fn workspace_memory(dims: [u32; 3], kernel: [u32; 3], bank: &KernelBank) -> u64 {
    let voxels = voxels_of(dims);
    let padded = [
        dims[0] + kernel[0] - 1,
        dims[1] + kernel[1] - 1,
        dims[2] + kernel[2] - 1,
    ];
    let stride = padded[0] + BLOCK;
    let padded_voxels = u64::from(stride) * u64::from(padded[1]) * u64::from(padded[2]);
    let taps = (bank.values().len() as u64 * 4).max(16);
    voxels * 4 * 5 + padded_voxels * 4 + taps + u64::from(padded[2]) * 4 + 64
}

fn pad_dispatch(padded: [u32; 3]) -> [u32; 3] {
    [
        padded[0].div_ceil(WORKGROUP[0]),
        padded[1].div_ceil(WORKGROUP[1]),
        padded[2].div_ceil(WORKGROUP[2]),
    ]
}

fn convolution_dispatch(dims: [u32; 3]) -> [u32; 3] {
    [
        dims[0].div_ceil(BLOCK).div_ceil(WORKGROUP[0]),
        dims[1].div_ceil(WORKGROUP[1]),
        dims[2].div_ceil(WORKGROUP[2]),
    ]
}

fn voxels_of(dims: [u32; 3]) -> u64 {
    u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2])
}

fn mirror(value: i32, limit: u32) -> u32 {
    if limit <= 1 {
        return 0;
    }
    let span = limit as i32;
    let mut mirrored = value;
    if mirrored < 0 {
        mirrored = -mirrored;
    } else if mirrored >= span {
        mirrored = 2 * (span - 1) - mirrored;
    }
    mirrored.clamp(0, span - 1) as u32
}
