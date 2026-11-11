use std::collections::HashSet;

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
    key: Option<u64>,
    slots: HashSet<(usize, usize, u32)>,
    entries: Vec<BakeEntry>,
    pending: bool,
}

impl BakeQueue {
    pub fn new() -> Self {
        Self {
            key: None,
            slots: HashSet::new(),
            entries: Vec::new(),
            pending: false,
        }
    }

    pub fn forget(&mut self) {
        self.key = None;
        self.slots.clear();
        self.pending = true;
    }

    pub fn touch(&mut self) {
        self.pending = true;
    }

    pub fn collect(
        &mut self,
        key: u64,
        atlas: &Atlas,
        levels: &[usize],
        time: u32,
        occupancy: &mut Occupancy,
        queue: &Queue,
    ) -> &[BakeEntry] {
        if self.key != Some(key) {
            self.key = Some(key);
            self.slots.clear();
            self.pending = true;
            occupancy.clear(queue);
        }
        if !self.pending {
            return &[];
        }
        self.pending = false;
        self.entries.clear();
        let mut residents = HashSet::new();
        for (band, level) in levels.iter().enumerate() {
            for resident in atlas.residents(time, *level) {
                let Some(slot) = resident.slot else {
                    continue;
                };
                let key = (*level, resident.index, slot);
                residents.insert(key);
                if !self.slots.contains(&key) {
                    self.entries.push(BakeEntry {
                        band: band as u32,
                        slot,
                        brick: resident.index as u32,
                        reserved: 0,
                    });
                }
            }
        }
        self.slots = residents;
        &self.entries
    }
}
