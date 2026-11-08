use bytemuck::{Pod, Zeroable, bytes_of};
use grana_volume::{BrickGrid, Pyramid};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePipeline, ComputePipelineDescriptor, Device, Queue,
    ShaderModuleDescriptor, ShaderSource, util::DeviceExt,
};

use crate::layout::Layouts;

pub(crate) const CENSUS_LEVELS: usize = 32;
const WORKGROUP: u32 = 64;
const SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/frustum.wgsl"),
    "\n",
    include_str!("shader/census.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CensusData {
    grid: [u32; 4],
    dims: [f32; 4],
    box_min: [f32; 4],
    box_size: [f32; 4],
    index: [u32; 4],
}

const _: () = assert!(size_of::<CensusData>() == 80);

struct LevelCensus {
    group: BindGroup,
    dispatch: u32,
}

pub(crate) struct Census {
    pipeline: ComputePipeline,
    counts: Buffer,
    levels: Vec<LevelCensus>,
}

impl Census {
    pub fn new(device: &Device, layouts: &Layouts, pyramid: &Pyramid) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.census"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.census"),
            layout: Some(&layouts.census),
            module: &module,
            entry_point: Some("census_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let counts = device.create_buffer(&BufferDescriptor {
            label: Some("grana.census.counts"),
            size: (CENSUS_LEVELS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let levels = pyramid
            .levels()
            .iter()
            .take(CENSUS_LEVELS)
            .enumerate()
            .map(|(level_index, level)| {
                let grid = BrickGrid::new(level.extent());
                let bricks = grid.counts();
                let extent = level.extent();
                let world = level.world_extent();
                let origin = level.origin();
                let data = CensusData {
                    grid: [bricks[0], bricks[1], bricks[2], grid.count() as u32],
                    dims: [extent.x() as f32, extent.y() as f32, extent.z() as f32, 0.0],
                    box_min: [origin[0], origin[1], origin[2], 0.0],
                    box_size: [world[0], world[1], world[2], 0.0],
                    index: [level_index as u32, 0, 0, 0],
                };
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("grana.census.level"),
                    contents: bytes_of(&data),
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                });
                let group = device.create_bind_group(&BindGroupDescriptor {
                    label: Some("grana.census.level.group"),
                    layout: &layouts.census_level,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: counts.as_entire_binding(),
                        },
                    ],
                });
                LevelCensus {
                    group,
                    dispatch: (grid.count() as u32).div_ceil(WORKGROUP),
                }
            })
            .collect();
        Self {
            pipeline,
            counts,
            levels,
        }
    }

    pub fn counts(&self) -> &Buffer {
        &self.counts
    }

    pub fn prepare(&self, queue: &Queue) {
        queue.write_buffer(&self.counts, 0, &[0u8; CENSUS_LEVELS * size_of::<u32>()]);
    }

    pub fn encode(&self, encoder: &mut CommandEncoder, scene: &BindGroup, first_level: usize) {
        if first_level >= self.levels.len() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.census.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene, &[]);
        for level in &self.levels[first_level..] {
            pass.set_bind_group(1, &level.group, &[]);
            pass.dispatch_workgroups(level.dispatch, 1, 1);
        }
    }
}
