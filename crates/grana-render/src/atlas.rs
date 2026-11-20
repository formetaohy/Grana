use std::collections::{BTreeMap, HashMap, HashSet};

use grana_volume::PADDED_SIDE;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Device,
    Sampler, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension,
};

use grana_volume::CHANNEL_LIMIT;

use grana_volume::{PACK_LIMIT, components};

const HOLLOW_FRAMES: usize = 8;

pub(crate) const STRUCTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

pub(crate) const STRUCTURE_BYTES: u64 = (grana_volume::PADDED_SIDE as u64).pow(3) * 4;

fn pack_format(components: usize) -> TextureFormat {
    match components {
        1 => TextureFormat::R16Unorm,
        2 => TextureFormat::Rg16Unorm,
        3 | 4 => TextureFormat::Rgba16Unorm,
        _ => unreachable!("a channel pack holds one to four channels"),
    }
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
    held: bool,
    referenced: bool,
}

impl Slot {
    const EMPTY: Self = Self {
        brick: None,
        held: false,
        referenced: false,
    };
}

pub(crate) struct AtlasBinding<'a> {
    pub layout: &'a BindGroupLayout,
    pub source: &'a BindGroupLayout,
    pub dummy: &'a TextureView,
    pub sampler: &'a Sampler,
}

pub(crate) struct Atlas {
    textures: Vec<Texture>,
    structure_storage: TextureView,
    source: BindGroup,
    group: BindGroup,
    slot_grid: [u32; 3],
    free: Vec<u32>,
    slots: Vec<Slot>,
    hand: usize,
    held: Vec<u32>,
    versions: Vec<u32>,
    writes: u64,
    now: u32,
    identity: HashMap<(u32, usize, usize), u32>,
    frames: BTreeMap<(u32, usize), Vec<u32>>,
    hollow: HashSet<(u32, usize, usize)>,
    hollowed: BTreeMap<(u32, usize), Vec<usize>>,
    remembered: Vec<u32>,
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
        let packs = grana_volume::pack_count(channel_count);
        let widths = (0..packs)
            .map(|pack| components(channel_count, pack))
            .collect::<Vec<_>>();
        let textures = widths
            .iter()
            .map(|width| {
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
                    format: pack_format(*width),
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
            structure_storage,
            source,
            group,
            slot_grid,
            free: (0..capacity as u32).rev().collect(),
            slots: vec![Slot::EMPTY; capacity],
            hand: 0,
            held: Vec::new(),
            versions: vec![0; capacity],
            writes: 0,
            now: 0,
            identity: HashMap::new(),
            frames: BTreeMap::new(),
            hollow: HashSet::new(),
            hollowed: BTreeMap::new(),
            remembered: Vec::new(),
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

    pub fn affect(
        &mut self,
        level: usize,
        index: usize,
        held: &mut Vec<ResidentBrick>,
        released: &mut Vec<u32>,
    ) {
        held.clear();
        released.clear();
        for (time, framed) in self.frames.keys().copied() {
            if framed == level && self.identity.contains_key(&(time, level, index)) {
                held.push(ResidentBrick { time, level, index });
            }
        }
        for time in self.remembered.clone() {
            if self.unhollow(time, level, index) {
                released.push(time);
            }
        }
    }

    fn unhollow(&mut self, time: u32, level: usize, index: usize) -> bool {
        if !self.hollow.remove(&(time, level, index)) {
            return false;
        }
        let empty = {
            let hollowed = self.hollowed.entry((time, level)).or_default();
            if let Some(position) = hollowed.iter().position(|hollow| *hollow == index) {
                hollowed.remove(position);
            }
            hollowed.is_empty()
        };
        if empty {
            self.hollowed.remove(&(time, level));
        }
        true
    }

    pub fn evict_other_frames(&mut self, time: u32) -> Vec<ResidentBrick> {
        let mut evicted = Vec::new();
        for (frame, slots) in std::mem::take(&mut self.frames) {
            if frame.0 == time {
                self.frames.insert(frame, slots);
                continue;
            }
            for slot in slots {
                let brick = self.slots[slot as usize]
                    .brick
                    .expect("a framed slot holds a brick");
                self.slots[slot as usize] = Slot::EMPTY;
                self.identity
                    .remove(&(brick.time, brick.level, brick.index));
                self.free.push(slot);
                evicted.push(brick);
            }
        }
        evicted
    }

    pub fn is_hollow(&self, time: u32, level: usize, index: usize) -> bool {
        self.hollow.contains(&(time, level, index))
    }

    pub fn assign_hollow(&mut self, time: u32, level: usize, index: usize) {
        if !self.hollow.insert((time, level, index)) {
            return;
        }
        self.hollowed.entry((time, level)).or_default().push(index);
        self.remember(time);
    }

    pub fn remember(&mut self, time: u32) {
        if self.remembered.contains(&time) {
            return;
        }
        self.remembered.push(time);
        while self.remembered.len() > HOLLOW_FRAMES {
            let dropped = self.remembered.remove(0);
            self.hollow.retain(|(held, _, _)| *held != dropped);
            self.hollowed.retain(|(held, _), _| *held != dropped);
        }
    }

    pub fn reset(&mut self) {
        let capacity = self.slots.len() as u32;
        self.free = (0..capacity).rev().collect();
        self.slots.fill(Slot::EMPTY);
        self.hand = 0;
        self.held.clear();
        self.identity.clear();
        self.frames.clear();
        self.hollow.clear();
        self.hollowed.clear();
        self.remembered.clear();
    }

    pub fn residents(&self, time: u32, level: usize, out: &mut Vec<Resident>) {
        out.clear();
        if let Some(slots) = self.frames.get(&(time, level)) {
            for slot in slots {
                out.push(Resident {
                    index: self.slots[*slot as usize]
                        .brick
                        .expect("a framed slot holds a brick")
                        .index,
                    slot: Some(*slot),
                });
            }
        }
        if let Some(indices) = self.hollowed.get(&(time, level)) {
            for index in indices {
                out.push(Resident {
                    index: *index,
                    slot: None,
                });
            }
        }
    }

    pub fn touch(&mut self, time: u32, level: usize, indices: impl Iterator<Item = u32>) {
        for index in indices {
            let Some(slot) = self.identity.get(&(time, level, index as usize)).copied() else {
                continue;
            };
            self.hold(slot, true);
            self.slots[slot as usize].referenced = true;
        }
    }

    fn hold(&mut self, slot: u32, hold: bool) {
        let placed = &mut self.slots[slot as usize];
        if !hold {
            placed.held = false;
            return;
        }
        if !placed.held {
            placed.held = true;
            self.held.push(slot);
        }
    }

    pub fn clear_holds(&mut self) {
        for slot in self.held.drain(..) {
            self.slots[slot as usize].held = false;
        }
    }

    pub fn allocate(&mut self, now: u32, request: u32) -> Option<(u32, Option<ResidentBrick>)> {
        self.now = now;
        if let Some(slot) = self.free.pop() {
            return Some((slot, None));
        }
        let victim = self.victim(request <= now)?;
        let brick = self.slots[victim as usize]
            .brick
            .expect("the eviction candidate holds a brick");
        self.slots[victim as usize] = Slot::EMPTY;
        self.identity
            .remove(&(brick.time, brick.level, brick.index));
        self.forget(brick.time, brick.level, victim);
        Some((victim, Some(brick)))
    }

    fn forget(&mut self, time: u32, level: usize, slot: u32) {
        let Some(slots) = self.frames.get_mut(&(time, level)) else {
            return;
        };
        if let Some(position) = slots.iter().position(|framed| *framed == slot) {
            slots.remove(position);
        }
        if slots.is_empty() {
            self.frames.remove(&(time, level));
        }
    }

    fn victim(&mut self, allow_future: bool) -> Option<u32> {
        self.sweep(false, true)
            .or_else(|| self.sweep(false, false))
            .or_else(|| allow_future.then(|| self.sweep(true, true)).flatten())
            .or_else(|| allow_future.then(|| self.sweep(true, false)).flatten())
    }

    fn sweep(&mut self, future: bool, referenced: bool) -> Option<u32> {
        for _ in 0..self.slots.len() {
            let slot = self.hand;
            self.hand = (self.hand + 1) % self.slots.len();
            let candidate = self.slots[slot];
            if candidate.brick.is_none() || candidate.held {
                continue;
            }
            if self.future(slot as u32) != future {
                continue;
            }
            if !referenced {
                return Some(slot as u32);
            }
            if candidate.referenced {
                self.slots[slot].referenced = false;
            } else {
                return Some(slot as u32);
            }
        }
        None
    }

    fn future(&self, slot: u32) -> bool {
        self.slots[slot as usize]
            .brick
            .expect("a placed slot holds a brick")
            .time
            > self.now
    }

    pub fn assign(&mut self, slot: u32, brick: ResidentBrick, hold: bool) {
        self.release(slot);
        self.unhollow(brick.time, brick.level, brick.index);
        self.identity
            .insert((brick.time, brick.level, brick.index), slot);
        self.frames
            .entry((brick.time, brick.level))
            .or_default()
            .push(slot);
        let placed = &mut self.slots[slot as usize];
        placed.brick = Some(brick);
        placed.referenced = true;
        self.hold(slot, hold);
        self.written(slot);
    }

    fn release(&mut self, slot: u32) {
        let Some(previous) = self.slots[slot as usize].brick else {
            return;
        };
        self.identity
            .remove(&(previous.time, previous.level, previous.index));
        self.forget(previous.time, previous.level, slot);
    }

    pub fn texture(&self, pack: usize) -> &Texture {
        assert!(
            pack < self.textures.len(),
            "a brick pack names one of the atlas textures"
        );
        &self.textures[pack]
    }

    pub fn origin(&self, slot: u32) -> wgpu::Origin3d {
        assert!(
            (slot as usize) < self.slots.len(),
            "a brick slot lies inside the atlas"
        );
        let origin = slot_origin(slot, self.slot_grid);
        wgpu::Origin3d {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        }
    }
}

fn slot_origin(slot: u32, slot_grid: [u32; 3]) -> [u32; 3] {
    let layer = slot / (slot_grid[0] * slot_grid[1]);
    let remainder = slot % (slot_grid[0] * slot_grid[1]);
    let row = remainder / slot_grid[0];
    let column = remainder % slot_grid[0];
    [column * PADDED_SIDE, row * PADDED_SIDE, layer * PADDED_SIDE]
}
