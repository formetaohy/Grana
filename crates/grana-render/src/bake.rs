use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, ComputePassDescriptor, ComputePipeline,
    ComputePipelineDescriptor, Device, Queue, ShaderModuleDescriptor, ShaderSource, TextureView,
};

use crate::atlas::Atlas;
use crate::layout::Layouts;

const SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/optical.wgsl"),
    "\n",
    include_str!("shader/structure.wgsl")
);

pub(crate) const BAKE_TILES: u32 = 9;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct BakeEntry {
    pub band: u32,
    pub slot: u32,
    pub brick: u32,
    pub reserved: u32,
}

pub(crate) struct Bake {
    pipeline: ComputePipeline,
    entries: Buffer,
    capacity: usize,
    group: BindGroup,
}

impl Bake {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        structure: &TextureView,
        capacity: usize,
    ) -> Self {
        assert!(capacity > 0, "a bake pass stages at least one slot");
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.bake"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.bake"),
            layout: Some(&layouts.bake_pipeline),
            module: &module,
            entry_point: Some("bake_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let entries = device.create_buffer(&BufferDescriptor {
            label: Some("grana.bake.entries"),
            size: (capacity * size_of::<BakeEntry>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.bake.group"),
            layout: &layouts.bake,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: entries.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(structure),
                },
            ],
        });
        Self {
            pipeline,
            entries,
            capacity,
            group,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn stage(&self, queue: &Queue, entries: &[BakeEntry]) {
        assert!(
            entries.len() <= self.capacity,
            "a bake pass stages at most {} slots",
            self.capacity
        );
        queue.write_buffer(&self.entries, 0, bytemuck::cast_slice(entries));
    }

    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        count: u32,
        scene: &BindGroup,
        level: &BindGroup,
        atlas: &BindGroup,
    ) {
        if count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("grana.bake.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene, &[]);
        pass.set_bind_group(1, level, &[]);
        pass.set_bind_group(2, atlas, &[]);
        pass.set_bind_group(5, &self.group, &[]);
        pass.dispatch_workgroups(BAKE_TILES, BAKE_TILES, BAKE_TILES * count);
    }
}

pub(crate) struct BakeQueue {
    staged: HashMap<u32, u64>,
    entries: Vec<BakeEntry>,
}

impl BakeQueue {
    pub fn new() -> Self {
        Self {
            staged: HashMap::new(),
            entries: Vec::new(),
        }
    }

    pub fn forget(&mut self) {
        self.staged.clear();
    }

    pub fn collect(
        &mut self,
        atlas: &Atlas,
        levels: &[usize],
        time: u32,
        capacity: usize,
    ) -> &[BakeEntry] {
        assert!(capacity > 0, "a bake pass stages at least one slot");
        self.entries.clear();
        'bands: for (band, level) in levels.iter().enumerate() {
            for resident in atlas.residents(time, *level) {
                let Some(slot) = resident.slot else {
                    continue;
                };
                let key = stamp(time, *level as u32, resident.index as u32);
                if self.staged.get(&slot) == Some(&key) {
                    continue;
                }
                if self.entries.len() == capacity {
                    break 'bands;
                }
                self.staged.insert(slot, key);
                self.entries.push(BakeEntry {
                    band: band as u32,
                    slot,
                    brick: resident.index as u32,
                    reserved: 0,
                });
            }
        }
        &self.entries
    }
}

fn stamp(time: u32, level: u32, brick: u32) -> u64 {
    let mut hash = u64::from(time) ^ u64::from(level).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= u64::from(brick).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^ (hash >> 33)
}
