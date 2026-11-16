use std::collections::{BTreeMap, HashMap, HashSet};

use bytemuck::cast_slice;
use grana_volume::PADDED_SIDE;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Device,
    Queue, Sampler, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension,
};

use grana_volume::CHANNEL_LIMIT;

pub(crate) const PACK: usize = 4;
pub(crate) const PACK_LIMIT: usize = CHANNEL_LIMIT / PACK;

pub(crate) const STRUCTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

pub(crate) const STRUCTURE_BYTES: u64 = (grana_volume::PADDED_SIDE as u64).pow(3) * 4;

const _: () = assert!(
    PACK_LIMIT == 4,
    "the volume shaders bind four channel packs"
);

pub(crate) fn pack_components(channels: usize, pack: usize) -> usize {
    assert!(pack * PACK < channels, "a pack holds at least one channel");
    (channels - pack * PACK).min(PACK)
}

pub(crate) fn pack_texels(components: usize) -> usize {
    match components {
        1 => 1,
        2 => 2,
        3 | 4 => 4,
        _ => unreachable!("a channel pack holds one to four channels"),
    }
}

fn pack_format(components: usize) -> TextureFormat {
    match components {
        1 => TextureFormat::R16Unorm,
        2 => TextureFormat::Rg16Unorm,
        3 | 4 => TextureFormat::Rgba16Unorm,
        _ => unreachable!("a channel pack holds one to four channels"),
    }
}

fn pack_bytes(texels: usize) -> u64 {
    texels as u64 * size_of::<u16>() as u64
}

#[derive(Clone, Copy)]
pub(crate) struct ResidentBrick {
    pub time: u32,
    pub level: usize,
    pub index: usize,
}

pub(crate) struct Resident {
    pub index: usize,
    pub slot: Option<u32>,
}

#[derive(Clone, Copy)]
struct Slot {
    brick: Option<ResidentBrick>,
    used: u64,
    held: bool,
    order: u32,
    future: bool,
}

impl Slot {
    const EMPTY: Self = Self {
        brick: None,
        used: 0,
        held: false,
        order: 0,
        future: false,
    };
}

#[derive(Default)]
struct Bucket {
    current: Vec<u32>,
    future: Vec<u32>,
}

pub(crate) struct AtlasBinding<'a> {
    pub layout: &'a BindGroupLayout,
    pub source: &'a BindGroupLayout,
    pub dummy: &'a TextureView,
    pub sampler: &'a Sampler,
}

pub(crate) struct Atlas {
    textures: Vec<Texture>,
    components: Vec<usize>,
    structure_storage: TextureView,
    source: BindGroup,
    group: BindGroup,
    slot_grid: [u32; 3],
    free: Vec<u32>,
    slots: Vec<Slot>,
    buckets: BTreeMap<u64, Bucket>,
    versions: Vec<u32>,
    writes: u64,
    now: u32,
    identity: HashMap<(u32, usize, usize), u32>,
    hollow: HashSet<(u32, usize, usize)>,
    pruned: Option<[u32; 2]>,
}

impl Atlas {
    pub fn new(
        device: &Device,
        binding: AtlasBinding<'_>,
        channel_count: usize,
        requested: u64,
        max_dimension: u32,
    ) -> Self {
        let AtlasBinding {
            layout,
            source: source_layout,
            dummy,
            sampler,
        } = binding;
        let slot_side = PADDED_SIDE;
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channel_count),
            "an atlas binds one to {} channels",
            CHANNEL_LIMIT
        );
        assert!(
            max_dimension >= slot_side,
            "the adapter cannot hold a padded brick"
        );
        let limit = (max_dimension / slot_side) as usize;
        let requested = requested.max(1) as usize;
        let span = ((requested as f64).cbrt().ceil() as usize).clamp(1, limit);
        let depth = requested.div_ceil(span * span).clamp(1, limit);
        let slot_grid = [span as u32, span as u32, depth as u32];
        let capacity = span * span * depth;
        let dims = slot_grid.map(|count| count * slot_side);
        let packs = channel_count.div_ceil(PACK);
        let components = (0..packs)
            .map(|pack| pack_components(channel_count, pack))
            .collect::<Vec<_>>();
        let textures = components
            .iter()
            .map(|components| {
                device.create_texture(&TextureDescriptor {
                    label: Some("grana.atlas"),
                    size: wgpu::Extent3d {
                        width: dims[0],
                        height: dims[1],
                        depth_or_array_layers: dims[2],
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D3,
                    format: pack_format(*components),
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .collect::<Vec<_>>();
        let views = textures
            .iter()
            .map(|texture| {
                texture.create_view(&TextureViewDescriptor {
                    label: Some("grana.atlas.view"),
                    dimension: Some(TextureViewDimension::D3),
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        let mut bindings = views.iter().collect::<Vec<&TextureView>>();
        bindings.resize(PACK_LIMIT, dummy);
        let structure = device.create_texture(&TextureDescriptor {
            label: Some("grana.atlas.structure"),
            size: wgpu::Extent3d {
                width: dims[0],
                height: dims[1],
                depth_or_array_layers: dims[2],
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: STRUCTURE_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let structure_view = structure.create_view(&TextureViewDescriptor {
            label: Some("grana.atlas.structure.view"),
            dimension: Some(TextureViewDimension::D3),
            ..Default::default()
        });
        let structure_storage = structure.create_view(&TextureViewDescriptor {
            label: Some("grana.atlas.structure.storage"),
            dimension: Some(TextureViewDimension::D3),
            ..Default::default()
        });
        let source = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.atlas.source.group"),
            layout: source_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureViewArray(&bindings),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.atlas.group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureViewArray(&bindings),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&structure_view),
                },
            ],
        });
        Self {
            textures,
            components,
            structure_storage,
            source,
            group,
            slot_grid,
            free: (0..capacity as u32).rev().collect(),
            slots: vec![Slot::EMPTY; capacity],
            buckets: BTreeMap::new(),
            versions: vec![0; capacity],
            writes: 0,
            now: 0,
            identity: HashMap::new(),
            hollow: HashSet::new(),
            pruned: None,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn source(&self) -> &BindGroup {
        &self.source
    }

    pub fn structure(&self) -> &TextureView {
        &self.structure_storage
    }

    pub fn slot_grid(&self) -> [u32; 3] {
        self.slot_grid
    }

    pub fn slots(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, time: u32, level: usize, index: usize) -> Option<u32> {
        self.identity.get(&(time, level, index)).copied()
    }

    pub fn version(&self, slot: u32) -> u32 {
        self.versions[slot as usize]
    }

    pub fn written(&mut self, slot: u32) {
        self.versions[slot as usize] = self.versions[slot as usize].wrapping_add(1);
        self.writes = self.writes.wrapping_add(1);
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn held(&self, level: usize, index: usize) -> Vec<ResidentBrick> {
        self.slots
            .iter()
            .filter_map(|entry| {
                let brick = entry.brick?;
                (brick.level == level && brick.index == index).then_some(brick)
            })
            .collect()
    }

    pub fn release_hollow(&mut self, level: usize, index: usize) -> Vec<u32> {
        let released = self
            .hollow
            .iter()
            .filter(|(_, hollow_level, hollow_index)| {
                *hollow_level == level && *hollow_index == index
            })
            .map(|(time, _, _)| *time)
            .collect::<Vec<_>>();
        for time in &released {
            self.hollow.remove(&(*time, level, index));
        }
        released
    }

    pub fn evict_other_frames(&mut self, time: u32) -> Vec<ResidentBrick> {
        let mut evicted = Vec::new();
        for slot in 0..self.slots.len() {
            let Some(brick) = self.slots[slot].brick else {
                continue;
            };
            if brick.time == time {
                continue;
            }
            self.unplace(slot as u32);
            self.slots[slot].brick = None;
            self.identity
                .remove(&(brick.time, brick.level, brick.index));
            self.free.push(slot as u32);
            evicted.push(brick);
        }
        evicted
    }

    pub fn is_hollow(&self, time: u32, level: usize, index: usize) -> bool {
        self.hollow.contains(&(time, level, index))
    }

    pub fn assign_hollow(&mut self, time: u32, level: usize, index: usize) {
        self.hollow.insert((time, level, index));
    }

    pub fn prune(&mut self, frames: [u32; 2]) {
        if self.pruned == Some(frames) {
            return;
        }
        self.hollow
            .retain(|(time, _, _)| *time == frames[0] || *time == frames[1]);
        self.pruned = Some(frames);
    }

    pub fn reset(&mut self) {
        let capacity = self.slots.len() as u32;
        self.free = (0..capacity).rev().collect();
        self.slots.fill(Slot::EMPTY);
        self.buckets.clear();
        self.identity.clear();
        self.hollow.clear();
        self.pruned = None;
    }

    pub fn residents(&self, time: u32, level: usize) -> Vec<Resident> {
        let mut residents = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                let brick = entry.brick?;
                (brick.time == time && brick.level == level).then_some(Resident {
                    index: brick.index,
                    slot: Some(slot as u32),
                })
            })
            .collect::<Vec<_>>();
        residents.extend(
            self.hollow
                .iter()
                .filter(|(hollow_time, hollow_level, _)| {
                    *hollow_time == time && *hollow_level == level
                })
                .map(|(_, _, index)| Resident {
                    index: *index,
                    slot: None,
                }),
        );
        residents
    }

    pub fn touch(&mut self, time: u32, level: usize, indices: impl Iterator<Item = u32>, pin: u64) {
        for index in indices {
            let Some(slot) = self.identity.get(&(time, level, index as usize)).copied() else {
                continue;
            };
            let brick = self.slots[slot as usize]
                .brick
                .expect("a named slot holds a brick");
            self.unplace(slot);
            self.slots[slot as usize].held = true;
            self.place(slot, pin, brick.time > self.now);
        }
    }

    pub fn clear_holds(&mut self) {
        for slot in &mut self.slots {
            slot.held = false;
        }
    }

    pub fn allocate(
        &mut self,
        pin: u64,
        now: u32,
        request: u32,
        demand: bool,
    ) -> Option<(u32, Option<ResidentBrick>)> {
        self.retime(now);
        if let Some(slot) = self.free.pop() {
            return Some((slot, None));
        }
        let victim = self.victim(pin, request <= now, demand)?;
        let brick = self.slots[victim as usize]
            .brick
            .expect("the eviction candidate holds a brick");
        self.unplace(victim);
        self.slots[victim as usize].brick = None;
        self.identity
            .remove(&(brick.time, brick.level, brick.index));
        Some((victim, Some(brick)))
    }

    fn victim(&self, pin: u64, allow_future: bool, demand: bool) -> Option<u32> {
        self.oldest(pin, false)
            .or_else(|| demand.then(|| self.newest(pin, false)).flatten())
            .or_else(|| allow_future.then(|| self.oldest(pin, true)).flatten())
            .or_else(|| {
                (demand && allow_future)
                    .then(|| self.newest(pin, true))
                    .flatten()
            })
    }

    fn oldest(&self, pin: u64, future: bool) -> Option<u32> {
        for (used, bucket) in &self.buckets {
            if *used >= pin {
                break;
            }
            let held = if future {
                &bucket.future
            } else {
                &bucket.current
            };
            if let Some(slot) = held.first() {
                return Some(*slot);
            }
        }
        None
    }

    fn newest(&self, pin: u64, future: bool) -> Option<u32> {
        let bucket = self.buckets.get(&pin)?;
        let held = if future {
            &bucket.future
        } else {
            &bucket.current
        };
        held.iter()
            .copied()
            .find(|slot| !self.slots[*slot as usize].held)
    }

    fn place(&mut self, slot: u32, used: u64, future: bool) {
        let bucket = self.buckets.entry(used).or_default();
        let held = if future {
            &mut bucket.future
        } else {
            &mut bucket.current
        };
        let order = held.len() as u32;
        held.push(slot);
        let placed = &mut self.slots[slot as usize];
        placed.used = used;
        placed.future = future;
        placed.order = order;
    }

    fn unplace(&mut self, slot: u32) {
        let place = self.slots[slot as usize];
        let empty = {
            let bucket = self
                .buckets
                .get_mut(&place.used)
                .expect("a slot keeps its usage bucket");
            let held = if place.future {
                &mut bucket.future
            } else {
                &mut bucket.current
            };
            let last = held.pop().expect("a usage bucket holds its slots");
            if last != slot {
                held[place.order as usize] = last;
                self.slots[last as usize].order = place.order;
            }
            bucket.current.is_empty() && bucket.future.is_empty()
        };
        if empty {
            self.buckets.remove(&place.used);
        }
    }

    fn retime(&mut self, now: u32) {
        if self.now == now {
            return;
        }
        self.now = now;
        let moved = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, place)| {
                let brick = place.brick?;
                (place.future != (brick.time > now)).then_some(slot as u32)
            })
            .collect::<Vec<_>>();
        for slot in moved {
            let brick = self.slots[slot as usize]
                .brick
                .expect("a retimed slot holds a brick");
            let used = self.slots[slot as usize].used;
            self.unplace(slot);
            self.place(slot, used, brick.time > now);
        }
    }

    pub fn assign(&mut self, slot: u32, brick: ResidentBrick, pin: u64, hold: bool) {
        if let Some(previous) = self.slots[slot as usize].brick {
            self.unplace(slot);
            self.identity
                .remove(&(previous.time, previous.level, previous.index));
        }
        self.hollow.remove(&(brick.time, brick.level, brick.index));
        self.identity
            .insert((brick.time, brick.level, brick.index), slot);
        self.slots[slot as usize].brick = Some(brick);
        self.slots[slot as usize].held = hold;
        self.place(slot, pin, brick.time > self.now);
        self.written(slot);
    }

    pub fn write(&self, queue: &Queue, pack: usize, slot: u32, units: &[u16]) {
        let texels = pack_texels(self.components[pack]);
        assert_eq!(
            units.len() as u64,
            (PADDED_SIDE as u64).pow(3) * texels as u64,
            "a packed brick fills one slot"
        );
        let origin = slot_origin(slot, self.slot_grid);
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.textures[pack],
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                aspect: TextureAspect::All,
            },
            cast_slice(units),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PADDED_SIDE * pack_bytes(texels) as u32),
                rows_per_image: Some(PADDED_SIDE),
            },
            wgpu::Extent3d {
                width: PADDED_SIDE,
                height: PADDED_SIDE,
                depth_or_array_layers: PADDED_SIDE,
            },
        );
    }
}

fn slot_origin(slot: u32, slot_grid: [u32; 3]) -> [u32; 3] {
    let layer = slot / (slot_grid[0] * slot_grid[1]);
    let remainder = slot % (slot_grid[0] * slot_grid[1]);
    let row = remainder / slot_grid[0];
    let column = remainder % slot_grid[0];
    [column * PADDED_SIDE, row * PADDED_SIDE, layer * PADDED_SIDE]
}
