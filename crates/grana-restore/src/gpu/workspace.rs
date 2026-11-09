use wgpu::{BindGroup, Device, Queue};

use super::Dims;
use crate::gpu::pipeline::Pipelines;

pub(crate) struct Workspace {
    pub dims: [u32; 3],
    pub kernel: [u32; 3],
    pub padded: [u32; 3],
    pub stride: u32,
    pub groups: [BindGroup; 2],
    pub uniforms: wgpu::Buffer,
    pub taps: wgpu::Buffer,
    pub bases: wgpu::Buffer,
    pub measured: wgpu::Buffer,
    pub estimate: [wgpu::Buffer; 2],
    pub blurred: wgpu::Buffer,
    pub padded_values: wgpu::Buffer,
    pub staging: wgpu::Buffer,
    voxels: u64,
}

impl Workspace {
    pub fn new(
        device: &Device,
        queue: &Queue,
        pipelines: &Pipelines,
        dims: [u32; 3],
        kernel: [u32; 3],
        bases: &[u32],
        taps: &[f32],
    ) -> Self {
        let voxels = u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2]);
        let padded = [
            dims[0] + kernel[0] - 1,
            dims[1] + kernel[1] - 1,
            dims[2] + kernel[2] - 1,
        ];
        let stride = padded[0] + super::BLOCK;
        let padded_voxels = u64::from(stride) * u64::from(padded[1]) * u64::from(padded[2]);
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
        let taps_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.taps"),
            size: (taps.len() as u64 * 4).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let base_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.bases"),
            size: (bases.len() as u64 * 4).max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let padded_values = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.restore.padded"),
            size: padded_voxels * 4,
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
                taps_buffer.as_entire_binding(),
                base_buffer.as_entire_binding(),
                measured.as_entire_binding(),
                estimate[swap].as_entire_binding(),
                estimate[1 - swap].as_entire_binding(),
                blurred.as_entire_binding(),
                padded_values.as_entire_binding(),
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
                layout: &pipelines.layout,
                entries: &entries,
            })
        });
        queue.write_buffer(&taps_buffer, 0, bytemuck::cast_slice(taps));
        queue.write_buffer(&base_buffer, 0, bytemuck::cast_slice(bases));
        Self {
            dims,
            kernel,
            padded,
            stride,
            groups,
            uniforms,
            taps: taps_buffer,
            bases: base_buffer,
            measured,
            estimate,
            blurred,
            padded_values,
            staging,
            voxels,
        }
    }

    pub fn group(&self, current: usize) -> &BindGroup {
        &self.groups[current]
    }

    pub fn bytes(&self) -> u64 {
        self.voxels * 4
    }

    pub fn memory(&self) -> u64 {
        self.measured.size()
            + self.estimate[0].size()
            + self.estimate[1].size()
            + self.blurred.size()
            + self.padded_values.size()
            + self.staging.size()
            + self.taps.size()
            + self.bases.size()
            + self.uniforms.size()
    }

    pub fn fits_taps(&self, bytes: u64) -> bool {
        self.taps.size() >= bytes.max(16)
    }

    pub fn write_source(&self, queue: &Queue, values: &[f32]) {
        let bytes = bytemuck::cast_slice(values);
        queue.write_buffer(&self.measured, 0, bytes);
        queue.write_buffer(&self.estimate[0], 0, bytes);
    }

    pub fn write_uniforms(&self, queue: &Queue, uniform: &Dims) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniform));
    }
}
