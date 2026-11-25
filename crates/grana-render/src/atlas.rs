use std::collections::HashMap;

use grana_volume::PADDED_SIDE;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Device,
    Sampler, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension,
};

use grana_volume::CHANNEL_LIMIT;

use grana_volume::{PACK_LIMIT, components};

const HOLLOW_FRAMES: usize = 8;

const ABSENT: u32 = u32::MAX;

pub(crate) const STRUCTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

pub(crate) const STRUCTURE_SIDE: u32 = PADDED_SIDE / 2 + 2;

pub(crate) const STRUCTURE_BYTES: u64 = (STRUCTURE_SIDE as u64).pow(3) * 4;

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
    holds: u8,
    referenced: bool,
    previous: u32,
    next: u32,
}

impl Slot {
    const EMPTY: Self = Self {
        brick: None,
        holds: 0,
        referenced: false,
        previous: ABSENT,
        next: ABSENT,
    };
}

#[derive(Clone, Copy)]
struct Frame {
    head: u32,
    tail: u32,
}

struct Space {
    words: Vec<u64>,
    free: usize,
    cursor: usize,
}

impl Space {
    fn new(capacity: usize) -> Self {
        let mut words = vec![u64::MAX; capacity.div_ceil(u64::BITS as usize)];
        let held = capacity % u64::BITS as usize;
        if held != 0
            && let Some(last) = words.last_mut()
        {
            *last = (1u64 << held) - 1;
        }
        Self {
            words,
            free: capacity,
            cursor: 0,
        }
    }

    fn take(&mut self) -> Option<u32> {
        if self.free == 0 {
            return None;
        }
        let count = self.words.len();
        for offset in 0..count {
            let word = (self.cursor + offset) % count;
            let bits = self.words[word];
            if bits == 0 {
                continue;
            }
            let bit = bits.trailing_zeros() as usize;
            self.words[word] &= !(1u64 << bit);
            self.free -= 1;
            self.cursor = word;
            return Some((word * u64::BITS as usize + bit) as u32);
        }
        None
    }

    fn release(&mut self, slot: u32) {
        let index = slot as usize;
        assert!(
            self.words[index / u64::BITS as usize] & (1u64 << (index % u64::BITS as usize)) == 0,
            "a released brick slot was occupied"
        );
        self.words[index / u64::BITS as usize] |= 1u64 << (index % u64::BITS as usize);
        self.free += 1;
    }

    fn restore(&mut self, capacity: usize) {
        *self = Self::new(capacity);
    }
}

#[derive(Default)]
struct BrickFrame {
    words: Vec<u64>,
    count: usize,
}

impl BrickFrame {
    fn holds(&self, index: usize) -> bool {
        self.words
            .get(index / u64::BITS as usize)
            .is_some_and(|word| word & (1u64 << (index % u64::BITS as usize)) != 0)
    }

    fn set(&mut self, index: usize) -> bool {
        let word = index / u64::BITS as usize;
        if self.words.len() <= word {
            self.words.resize(word + 1, 0);
        }
        let bit = 1u64 << (index % u64::BITS as usize);
        if self.words[word] & bit != 0 {
            return false;
        }
        self.words[word] |= bit;
        self.count += 1;
        true
    }

    fn clear(&mut self, index: usize) -> bool {
        let word = index / u64::BITS as usize;
        if self.words.len() <= word {
            return false;
        }
        let bit = 1u64 << (index % u64::BITS as usize);
        if self.words[word] & bit == 0 {
            return false;
        }
        self.words[word] &= !bit;
        self.count -= 1;
        true
    }
}

#[derive(Default)]
struct Frames {
    frames: HashMap<(u32, usize), BrickFrame>,
}

impl Frames {
    fn holds(&self, time: u32, level: usize, index: usize) -> bool {
        self.frames
            .get(&(time, level))
            .is_some_and(|frame| frame.holds(index))
    }

    fn set(&mut self, time: u32, level: usize, index: usize) -> bool {
        self.frames.entry((time, level)).or_default().set(index)
    }

    fn clear(&mut self, time: u32, level: usize, index: usize) -> bool {
        let Some(frame) = self.frames.get_mut(&(time, level)) else {
            return false;
        };
        if !frame.clear(index) {
            return false;
        }
        if frame.count == 0 {
            self.frames.remove(&(time, level));
        }
        true
    }

    fn reset(&mut self) {
        self.frames.clear();
    }

    fn drop_frame(&mut self, time: u32) {
        self.frames.retain(|(held, _), _| *held != time);
    }

    fn retain_from(&mut self, time: u32) {
        self.frames.retain(|(held, _), _| *held >= time);
    }

    fn marks(&self, time: u32) -> usize {
        self.frames
            .iter()
            .filter(|((held, _), _)| *held == time)
            .map(|(_, frame)| frame.count)
            .sum()
    }

    fn words(&self, time: u32, level: usize) -> Option<&[u64]> {
        self.frames
            .get(&(time, level))
            .map(|frame| frame.words.as_slice())
    }
}

struct Hollows {
    frames: Frames,
    remembered: Vec<u32>,
}

impl Hollows {
    fn new() -> Self {
        Self {
            frames: Frames::default(),
            remembered: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.frames.reset();
        self.remembered.clear();
    }

    fn holds(&self, time: u32, level: usize, index: usize) -> bool {
        self.frames.holds(time, level, index)
    }

    fn assign(&mut self, time: u32, level: usize, index: usize) -> Vec<u32> {
        if self.frames.holds(time, level, index) {
            return Vec::new();
        }
        let forgotten = self.remember(time);
        self.frames.set(time, level, index);
        forgotten
    }

    fn release(&mut self, time: u32, level: usize, index: usize) -> bool {
        self.frames.clear(time, level, index)
    }

    fn remember(&mut self, time: u32) -> Vec<u32> {
        if self.remembered.contains(&time) {
            return Vec::new();
        }
        self.remembered.push(time);
        let mut forgotten = Vec::new();
        while self.remembered.len() > HOLLOW_FRAMES {
            let dropped = self.remembered.remove(0);
            self.frames.drop_frame(dropped);
            forgotten.push(dropped);
        }
        forgotten
    }

    fn words(&self, time: u32, level: usize) -> Option<&[u64]> {
        self.frames.words(time, level)
    }
}

impl crate::hollow::Source for Atlas {
    fn hollow(&self, time: u32, level: usize) -> Option<&[u64]> {
        self.hollow.words(time, level)
    }
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
    space: Space,
    slots: Vec<Slot>,
    frames: HashMap<(u32, usize), Frame>,
    hollow: Hollows,
    unavailable: Frames,
    hand: usize,
    hold_generation: u8,
    versions: Vec<u32>,
    writes: u64,
    evictions: u64,
    now: u32,
    identity: HashMap<(u32, usize, usize), u32>,
}

impl Atlas {
    pub fn new(
        device: &Device,
        binding: AtlasBinding<'_>,
        channel_count: usize,
        slots: crate::slots::Slots,
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
        let slot_grid = slots.grid();
        let capacity = slots.capacity();
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
        let structure_dims = slot_grid.map(|count| count * STRUCTURE_SIDE);
        let structure = device.create_texture(&TextureDescriptor {
            label: Some("grana.atlas.structure"),
            size: wgpu::Extent3d {
                width: structure_dims[0],
                height: structure_dims[1],
                depth_or_array_layers: structure_dims[2],
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
            space: Space::new(capacity),
            slots: vec![Slot::EMPTY; capacity],
            frames: HashMap::new(),
            hollow: Hollows::new(),
            unavailable: Frames::default(),
            hand: 0,
            hold_generation: 0,
            versions: vec![0; capacity],
            writes: 0,
            evictions: 0,
            now: 0,
            identity: HashMap::new(),
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

    pub fn evictions(&self) -> u64 {
        self.evictions
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
        for frame in self.frames.keys() {
            if frame.1 != level {
                continue;
            }
            if self.identity.contains_key(&(frame.0, level, index)) {
                held.push(ResidentBrick {
                    time: frame.0,
                    level,
                    index,
                });
            }
        }
        let remembered = self.hollow.remembered.clone();
        for time in remembered {
            if self.hollow.release(time, level, index) {
                released.push(time);
            }
        }
    }

    pub fn evict_other_frames(&mut self, time: u32) -> Vec<ResidentBrick> {
        let Self {
            slots,
            frames,
            space,
            identity,
            ..
        } = self;
        let doomed = frames
            .keys()
            .copied()
            .filter(|frame| frame.0 != time)
            .collect::<Vec<_>>();
        let mut evicted = Vec::new();
        for key in doomed {
            let Some(frame) = frames.remove(&key) else {
                continue;
            };
            let mut cursor = frame.head;
            while cursor != ABSENT {
                let slot = &mut slots[cursor as usize];
                let brick = slot.brick.take().expect("a framed slot holds a brick");
                let next = slot.next;
                *slot = Slot::EMPTY;
                identity.remove(&(brick.time, brick.level, brick.index));
                space.release(cursor);
                self.evictions += 1;
                evicted.push(brick);
                cursor = next;
            }
        }
        evicted
    }

    pub fn is_hollow(&self, time: u32, level: usize, index: usize) -> bool {
        self.hollow.holds(time, level, index)
    }

    pub fn assign_hollow(&mut self, time: u32, level: usize, index: usize) -> Vec<u32> {
        self.unavailable.clear(time, level, index);
        self.hollow.assign(time, level, index)
    }

    pub fn is_unavailable(&self, time: u32, level: usize, index: usize) -> bool {
        self.unavailable.holds(time, level, index)
    }

    pub fn assign_unavailable(&mut self, time: u32, level: usize, index: usize) {
        if self.hollow.holds(time, level, index) {
            return;
        }
        self.unavailable.set(time, level, index);
    }

    pub fn retry(&mut self, time: u32, level: usize, index: usize) {
        self.unavailable.clear(time, level, index);
    }

    pub fn retire(&mut self, time: u32) {
        self.unavailable.retain_from(time);
    }

    pub fn unavailable_marks(&self, time: u32) -> usize {
        self.unavailable.marks(time)
    }

    pub fn remember(&mut self, time: u32) -> Vec<u32> {
        self.hollow.remember(time)
    }

    pub fn reset(&mut self) {
        let capacity = self.slots.len();
        self.space.restore(capacity);
        self.slots.fill(Slot::EMPTY);
        self.frames.clear();
        self.hollow.clear();
        self.unavailable.reset();
        self.hand = 0;
        self.hold_generation = 0;
        self.identity.clear();
    }

    pub fn residents(&self, time: u32, level: usize, out: &mut Vec<Resident>) {
        out.clear();
        let mut cursor = self
            .frames
            .get(&(time, level))
            .map_or(ABSENT, |frame| frame.head);
        while cursor != ABSENT {
            let slot = &self.slots[cursor as usize];
            out.push(Resident {
                index: slot.brick.expect("a framed slot holds a brick").index,
                slot: Some(cursor),
            });
            cursor = slot.next;
        }
        if let Some(words) = self.hollow.words(time, level) {
            for (position, word) in words.iter().enumerate() {
                let mut bits = *word;
                while bits != 0 {
                    let bit = bits.trailing_zeros() as usize;
                    out.push(Resident {
                        index: position * u64::BITS as usize + bit,
                        slot: None,
                    });
                    bits &= bits - 1;
                }
            }
        }
    }

    pub fn touch(&mut self, time: u32, level: usize, indices: impl Iterator<Item = u32>) {
        for index in indices {
            let Some(slot) = self.identity.get(&(time, level, index as usize)).copied() else {
                continue;
            };
            self.hold(slot);
            self.slots[slot as usize].referenced = true;
        }
    }

    fn hold(&mut self, slot: u32) {
        self.slots[slot as usize].holds |= 1 << self.hold_generation;
    }

    fn held(&self, slot: u32) -> bool {
        self.slots[slot as usize].holds & (1 << self.hold_generation) != 0
    }

    pub fn advance_holds(&mut self) {
        self.hold_generation ^= 1;
        let bit = !(1 << self.hold_generation);
        for slot in &mut self.slots {
            slot.holds &= bit;
        }
    }

    pub fn reserve(
        &mut self,
        now: u32,
        count: usize,
        evicted: &mut Vec<ResidentBrick>,
    ) -> Vec<u32> {
        self.now = now;
        let mut slots = Vec::with_capacity(count);
        while slots.len() < count {
            if let Some(slot) = self.space.take() {
                slots.push(slot);
                continue;
            }
            let Some(victim) = self.victim() else {
                break;
            };
            if let Some(brick) = self.detach(victim) {
                evicted.push(brick);
            }
            slots.push(victim);
        }
        slots
    }

    pub fn vacancies(&self) -> usize {
        self.space.free
    }

    pub fn available(&self) -> usize {
        self.slots
            .iter()
            .enumerate()
            .filter(|(slot, held)| held.brick.is_none() || !self.held(*slot as u32))
            .count()
    }

    pub fn reserve_vacant(&mut self, count: usize) -> Vec<u32> {
        let mut slots = Vec::with_capacity(count.min(self.space.free));
        while slots.len() < count {
            let Some(slot) = self.space.take() else {
                break;
            };
            slots.push(slot);
        }
        slots
    }

    fn detach(&mut self, slot: u32) -> Option<ResidentBrick> {
        let Self {
            slots,
            frames,
            identity,
            ..
        } = self;
        let held = slots[slot as usize];
        let brick = held.brick?;
        self.evictions += 1;
        let key = (brick.time, brick.level);
        let (previous, next) = (held.previous, held.next);
        slots[slot as usize] = Slot::EMPTY;
        identity.remove(&(brick.time, brick.level, brick.index));
        if let Some(frame) = frames.get_mut(&key) {
            if previous == ABSENT {
                frame.head = next;
            } else {
                slots[previous as usize].next = next;
            }
            if next == ABSENT {
                frame.tail = previous;
            } else {
                slots[next as usize].previous = previous;
            }
            if frame.head == ABSENT {
                frames.remove(&key);
            }
        }
        Some(brick)
    }

    fn victim(&mut self) -> Option<u32> {
        self.sweep(true, true)
            .or_else(|| self.sweep(true, false))
            .or_else(|| self.sweep(false, true))
            .or_else(|| self.sweep(false, false))
    }

    fn sweep(&mut self, future: bool, referenced: bool) -> Option<u32> {
        for _ in 0..self.slots.len() {
            let slot = self.hand;
            self.hand = (self.hand + 1) % self.slots.len();
            let candidate = self.slots[slot];
            if candidate.brick.is_none() || self.held(slot as u32) {
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
        let Self {
            slots,
            frames,
            identity,
            hollow,
            unavailable,
            ..
        } = self;
        assert!(
            slots[slot as usize].brick.is_none(),
            "an allocated brick slot is vacant"
        );
        hollow.release(brick.time, brick.level, brick.index);
        unavailable.clear(brick.time, brick.level, brick.index);
        identity.insert((brick.time, brick.level, brick.index), slot);
        let frame = frames.entry((brick.time, brick.level)).or_insert(Frame {
            head: ABSENT,
            tail: ABSENT,
        });
        let tail = frame.tail;
        slots[slot as usize].previous = tail;
        slots[slot as usize].next = ABSENT;
        slots[slot as usize].brick = Some(brick);
        slots[slot as usize].referenced = true;
        if tail == ABSENT {
            frame.head = slot;
        } else {
            slots[tail as usize].next = slot;
        }
        frame.tail = slot;
        if hold {
            slots[slot as usize].holds |= 1 << self.hold_generation;
        }
        self.written(slot);
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
