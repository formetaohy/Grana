struct Workspace {
    uniforms: wgpu::Buffer,
    taps: wgpu::Buffer,
    measured: wgpu::Buffer,
    estimate: [wgpu::Buffer; 2],
    staging: wgpu::Buffer,
    groups: [wgpu::BindGroup; 2],
    dims: [u32; 3],
}

struct Pipelines {
    layout: wgpu::BindGroupLayout,
    blur: wgpu::ComputePipeline,
    correct: wgpu::ComputePipeline,
}

const WORKGROUP: [u32; 3] = [8, 8, 4];
const READ: wgpu::BufferBindingType = wgpu::BufferBindingType::Storage { read_only: true };
const WRITE: wgpu::BufferBindingType = wgpu::BufferBindingType::Storage { read_only: false };

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
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    workspace: Option<Workspace>,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        let headless = grana_gpu::Headless::new()?;
        let pipelines = Pipelines::new(&headless.device);
        Ok(Self {
            device: headless.device,
            queue: headless.queue,
            pipelines,
            workspace: None,
        })
    }

    pub fn restore(
        &mut self,
        dims: [u32; 3],
        source: &[f32],
        kernel: &grana_optics::Kernel,
        iterations: u32,
    ) -> Vec<f32> {
        self.prepare(dims, kernel);
        let workspace = self
            .workspace
            .as_ref()
            .expect("the restore workspace is ready");
        let mut values = source.to_vec();
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
        let dispatch = [
            dims[0].div_ceil(WORKGROUP[0]),
            dims[1].div_ceil(WORKGROUP[1]),
            dims[2].div_ceil(WORKGROUP[2]),
        ];
        for iteration in 0..iterations {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grana.restore"),
                });
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
            self.queue.submit([encoder.finish()]);
        }
        self.read(iterations)
    }

    fn read(&mut self, iterations: u32) -> Vec<f32> {
        let workspace = self
            .workspace
            .as_mut()
            .expect("the restore workspace is ready");
        let voxels = (workspace.dims[0] * workspace.dims[1] * workspace.dims[2]) as u64;
        let bytes = voxels * 4;
        let source = &workspace.estimate[(iterations % 2) as usize];
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.restore.readback"),
            });
        encoder.copy_buffer_to_buffer(source, 0, &workspace.staging, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let slice = workspace.staging.slice(..bytes);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the graphics device failed while restoring");
        receiver
            .recv()
            .expect("the restore mapping callback runs once")
            .expect("the restore staging buffer is mappable");
        let mapped = slice
            .get_mapped_range()
            .expect("the mapped restore buffer stays mapped");
        let values = bytemuck::cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        workspace.staging.unmap();
        values
    }

    fn prepare(&mut self, dims: [u32; 3], kernel: &grana_optics::Kernel) {
        let kernel_dims = kernel.dims();
        let uniform = Dims {
            volume: [dims[0], dims[1], dims[2], 0],
            kernel: [kernel_dims[0], kernel_dims[1], kernel_dims[2], 0],
        };
        if let Some(workspace) = self.workspace.as_ref() {
            self.queue
                .write_buffer(&workspace.uniforms, 0, bytemuck::bytes_of(&uniform));
            self.queue
                .write_buffer(&workspace.taps, 0, bytemuck::cast_slice(kernel.values()));
        }
        if self
            .workspace
            .as_ref()
            .is_some_and(|held| held.dims == dims)
        {
            return;
        }
        let device = &self.device;
        let voxels = (dims[0] as u64 * dims[1] as u64 * dims[2] as u64).max(1);
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
            size: (kernel.voxels() as u64 * 4).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.staging"),
            size: voxels * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&uniforms, 0, bytemuck::bytes_of(&uniform));
        self.queue
            .write_buffer(&taps, 0, bytemuck::cast_slice(kernel.values()));
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
        self.workspace = Some(Workspace {
            uniforms,
            taps,
            measured,
            estimate,
            staging,
            groups,
            dims,
        });
    }
}
