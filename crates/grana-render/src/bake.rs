use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, ComputePassDescriptor, ComputePipeline,
    ComputePipelineDescriptor, Device, Queue, ShaderModuleDescriptor, ShaderSource, TextureView,
};

use crate::atlas::Atlas;
use crate::layout::Layouts;
use crate::occupancy::Occupancy;

const SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/shadow_sample.wgsl"),
    "\n",
    include_str!("shader/brdf.wgsl"),
    "\n",
    include_str!("shader/bake.wgsl")
);

pub(crate) const BAKE_TILES: u32 = 9;

const CAPACITY: usize = 4096;

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
    group: BindGroup,
}

impl Bake {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        occupancy: &Buffer,
        baked: &TextureView,
    ) -> Self {
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
            size: (CAPACITY * size_of::<BakeEntry>()) as u64,
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
                    resource: BindingResource::TextureView(baked),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: occupancy.as_entire_binding(),
                },
            ],
        });
        Self {
            pipeline,
            entries,
            group,
        }
    }

    pub fn stage(&self, queue: &Queue, entries: &[BakeEntry]) {
        assert!(
            entries.len() <= CAPACITY,
            "a bake pass stages at most {CAPACITY} slots"
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
        shadow: &BindGroup,
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
        pass.set_bind_group(4, shadow, &[]);
        pass.set_bind_group(5, &self.group, &[]);
        pass.dispatch_workgroups(BAKE_TILES, BAKE_TILES, BAKE_TILES * count);
    }
}

pub(crate) struct BakeQueue {
    baked: HashMap<u32, u64>,
    entries: Vec<BakeEntry>,
    reset: bool,
}

impl BakeQueue {
    pub fn new() -> Self {
        Self {
            baked: HashMap::new(),
            entries: Vec::new(),
            reset: false,
        }
    }

    pub fn forget(&mut self) {
        self.baked.clear();
        self.reset = true;
    }

    pub fn collect(
        &mut self,
        material: u64,
        atlas: &Atlas,
        levels: &[usize],
        time: u32,
        occupancy: &mut Occupancy,
        queue: &Queue,
    ) -> &[BakeEntry] {
        let reset = std::mem::take(&mut self.reset);
        if reset {
            occupancy.clear(queue);
        }
        self.entries.clear();
        for (band, level) in levels.iter().enumerate() {
            for resident in atlas.residents(time, *level) {
                let Some(slot) = resident.slot else {
                    continue;
                };
                let key = mix(
                    mix(mix(material, u64::from(time)), *level as u64),
                    resident.index as u64,
                );
                if self.baked.get(&slot) == Some(&key) {
                    continue;
                }
                self.baked.insert(slot, key);
                if !reset {
                    occupancy.clear_slot(queue, slot);
                }
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

fn mix(a: u64, b: u64) -> u64 {
    let mut hash = a ^ b.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash
}
