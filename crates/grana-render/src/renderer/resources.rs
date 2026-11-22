use bytemuck::cast_slice;
use grana_volume::{
    BRICK_SIDE, BrickCoord, BrickGrid, BrickRequest, ChannelSet, Damage, PADDED_SIDE, Pack,
    PackedBrick, UNIT_MAXIMUM,
};

use crate::atlas::ResidentBrick;
use crate::brick::{BrickRecord, RECORD_BYTES};
use crate::occupancy::{CELL_WORDS, Occupancy};

use super::*;

const APRON: u32 = 1;

const MINIMUM_SLOTS: u64 = 2;

pub(super) struct Resources {
    pub(super) pyramid: Pyramid,
    pub(super) atlas: Atlas,
    pub(super) arena: BrickArena,
    pub(super) bricks: Vec<Option<BrickTable>>,
    pub(super) occupancy: Occupancy,
    pub(super) occupancy_group: BindGroup,
    pub(super) bake: Bake,
    pub(super) baked: BakeQueue,
    pub(super) upload: crate::upload::Upload,
    pub(super) staged: Vec<(usize, u32, usize)>,
    pub(super) level_group: BindGroup,
    pub(super) planner: Planner,
    pub(super) shadow: Shadow,
    pub(super) scene: crate::table::SceneTable,
    pub(super) channels: usize,
    pub(super) slot_bytes: u64,
    pub(super) summary: Option<signature::SceneSummary>,
}

impl Resources {
    pub(super) fn new(
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        sampler: &Sampler,
        dummy: &TextureView,
        budget: u64,
        pyramid: &Pyramid,
    ) -> Result<Self, String> {
        let least = least_budget(pyramid);
        let mut budget = budget;
        loop {
            let attempt = grana_gpu::scoped(device, || {
                Self::allocate(device, layouts, pipelines, sampler, dummy, budget, pyramid)
            });
            match attempt {
                Ok(resources) => return Ok(resources),
                Err(wgpu::Error::OutOfMemory { .. }) if budget > least => {
                    budget = (budget / 2).max(least);
                    grana_log::warn!(
                        target: "grana::render",
                        budget,
                        "the graphics device is short of memory, so the brick residency shrinks"
                    );
                }
                Err(wgpu::Error::OutOfMemory { .. }) => {
                    return Err(format!(
                        "the graphics device cannot hold the smallest brick residency of {:.1} MiB",
                        least as f64 / (1u64 << 20) as f64
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "the renderer could not allocate its dataset resources: {error}"
                    ));
                }
            }
        }
    }

    fn allocate(
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        sampler: &Sampler,
        dummy: &TextureView,
        budget: u64,
        pyramid: &Pyramid,
    ) -> Self {
        let channels = pyramid.channel_count();
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a dataset holds one to {CHANNEL_LIMIT} channels"
        );
        assert!(
            !pyramid.levels().is_empty(),
            "a pyramid holds at least one level"
        );
        let holdable = pyramid
            .levels()
            .iter()
            .map(|level| BrickGrid::new(level.extent()).count() as u64)
            .sum::<u64>();
        let slot_bytes = slot_bytes(pyramid);
        let requested = (budget / slot_bytes).max(1).min(holdable);
        let atlas = Atlas::new(
            device,
            crate::atlas::AtlasBinding {
                layout: &layouts.atlas,
                source: &layouts.source,
                dummy,
                sampler,
            },
            channels,
            requested,
            device.limits().max_texture_dimension_3d,
        );
        let slots = atlas.slots();
        let occupancy = Occupancy::new(device, slots);
        let bake = Bake::new(device, layouts, atlas.structure(), slots, pipelines);
        let limit = (budget / RECORD_SHARE / crate::brick::RECORD_BYTES) as usize;
        let arena = BrickArena::new(device, pyramid, limit)
            .expect("the record budget holds the coarsest level");
        let level_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.level.group"),
            layout: &layouts.level,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: arena.buffer().as_entire_binding(),
            }],
        });
        let occupancy_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.occupancy.group"),
            layout: &layouts.occupancy,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: occupancy.buffer().as_entire_binding(),
            }],
        });
        let mut planner = Planner::new(device, layouts, slots, pipelines);
        planner.bind_arena(device, layouts, &arena);
        let counts = pyramid
            .levels()
            .iter()
            .map(|level| BrickGrid::new(level.extent()).count())
            .collect();
        let shadow = Shadow::new(device, layouts, pipelines, counts);
        let scene = crate::table::SceneTable::new(device, layouts, channels);
        Self {
            pyramid: pyramid.clone(),
            atlas,
            arena,
            bricks: (0..pyramid.level_count()).map(|_| None).collect(),
            occupancy,
            occupancy_group,
            bake,
            baked: BakeQueue::new(),
            upload: crate::upload::Upload::new(),
            staged: Vec::new(),
            level_group,
            planner,
            shadow,
            scene,
            channels,
            slot_bytes,
            summary: None,
        }
    }

    pub(super) fn slots(&self) -> usize {
        self.atlas.slots()
    }

    pub(super) fn first_level(&self) -> usize {
        self.arena.first()
    }

    pub(super) fn invalidate(&mut self) {
        self.upload.discard();
        self.atlas.reset();
        self.baked.forget();
        self.shadow.forget();
        self.summary = None;
    }

    pub(super) fn scopes(&self, damages: &[Damage]) -> Vec<(Region, ChannelSet)> {
        damages
            .iter()
            .filter_map(|damage| {
                let channels = self.scope(damage.channels());
                (!channels.is_empty()).then(|| (damage.region(), channels))
            })
            .collect()
    }

    fn scope(&self, channels: ChannelSet) -> ChannelSet {
        channels
            .iter()
            .filter(|channel| *channel < self.channels)
            .fold(ChannelSet::NONE, |scope, channel| {
                let first = channel / PACK * PACK;
                (first..first + PACK)
                    .filter(|member| *member < self.channels)
                    .fold(scope, ChannelSet::with)
            })
    }

    pub(super) fn damage(
        &mut self,
        scopes: &[(Region, ChannelSet)],
        residence: &mut Residence,
        time: u32,
    ) {
        residence.clear_prefetch();
        let Self {
            pyramid,
            arena,
            atlas,
            bricks,
            upload,
            shadow,
            ..
        } = self;
        let refreshes = residence.refreshes();
        let mut released = Vec::new();
        let mut resident = Vec::new();
        let mut dropped = Vec::new();
        for brick in atlas.evict_other_frames(time) {
            if let Some(table) = bricks[brick.level].as_mut()
                && table.drop_record(arena, brick.index, brick.time)
            {
                shadow.mark(brick.level, brick.index);
            }
        }
        for (region, channels) in scopes {
            for level in arena.first()..pyramid.level_count() {
                let source = pyramid.level(level);
                let Some(ranges) = region.voxel_range(source) else {
                    continue;
                };
                let extent = source.extent();
                let limits = [extent.x(), extent.y(), extent.z()];
                let ranges = [0, 1, 2].map(|axis| {
                    ranges[axis].start.saturating_sub(APRON)
                        ..(ranges[axis].end + APRON).min(limits[axis])
                });
                let grid = BrickGrid::new(extent);
                let (first, last) = grid.cover(&ranges);
                for z in first[2]..=last[2] {
                    for y in first[1]..=last[1] {
                        for x in first[0]..=last[0] {
                            let coord = BrickCoord::new(x, y, z);
                            let index = grid.index(coord);
                            atlas.affect(level, index, &mut resident, &mut dropped);
                            for brick in &resident {
                                refreshes
                                    .insert(BrickRequest::of(brick.time, level, coord, *channels));
                            }
                            for time in &dropped {
                                released.push((*time, level, index));
                            }
                        }
                    }
                }
            }
        }
        for (brick_time, level, index) in released {
            if let Some(table) = bricks[level].as_mut()
                && table.drop_record(arena, index, brick_time)
            {
                shadow.mark(level, index);
            }
        }
        for table in bricks.iter_mut().flatten() {
            table.flush(upload, arena);
        }
        residence.revalidate();
    }

    pub(super) fn upload(
        &mut self,
        bricks: &[(BrickRequest, PackedBrick)],
        residence: &mut Residence,
        time: u32,
    ) -> usize {
        let Self {
            bricks: tables,
            atlas,
            arena,
            pyramid,
            channels,
            upload,
            staged,
            occupancy,
            shadow,
            ..
        } = self;
        let content = residence.content();
        let mut generation = residence.generation();
        let mut supplied = Vec::new();
        let mut released = Vec::new();
        let mut missing = Vec::new();
        let total = *channels;
        let mut values = [0.0f32; CELL_WORDS];
        {
            let (prefetched, refreshes) = residence.streams();
            for (position, (request, packed)) in bricks.iter().enumerate() {
                let BrickRequest {
                    time: held,
                    level,
                    coord,
                    channels: scope,
                } = *request;
                if held < time {
                    prefetched.remove(request);
                    continue;
                }
                if level >= pyramid.level_count()
                    || level < arena.first()
                    || held >= pyramid.frame_count()
                    || packed.channels() != total
                {
                    continue;
                }
                let extent = pyramid.level(level).extent();
                if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
                    || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
                    || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
                {
                    continue;
                }
                let grid = BrickGrid::new(extent);
                let index = grid.index(coord);
                prefetched.remove(request);
                let demand = held == time;
                let bound = tables[level]
                    .as_ref()
                    .is_some_and(|table| table.time() == Some(held));
                if let Some(slot) = atlas.slot(held, level, index) {
                    let refresh = refreshes.remove(request);
                    if refresh || !scope.covers(total) {
                        for (pack, block) in packed.packs() {
                            staged.push((pack, slot, position));
                            occupy(&mut values, occupancy, slot, pack, block);
                        }
                        occupancy.flush(upload, slot);
                        atlas.written(slot);
                        if held == time {
                            generation += 1;
                        }
                        if bound || held == time {
                            shadow.mark(level, index);
                        }
                    }
                    continue;
                }
                if !scope.covers(total) || atlas.is_hollow(held, level, index) {
                    continue;
                }
                let maximum = packed
                    .packs()
                    .map(|(_, block)| block.maximum())
                    .fold(0.0, f32::max);
                let current = held == time;
                let publish = current
                    && tables[level]
                        .as_ref()
                        .is_some_and(|table| table.time() == Some(held));
                if maximum <= 0.0 {
                    atlas.assign_hollow(held, level, index);
                    if publish {
                        if let Some(table) = tables[level].as_mut() {
                            table.set_record(arena, index, BrickRecord::hollow(held, content));
                        }
                        shadow.mark(level, index);
                    }
                    if current {
                        generation += 1;
                    }
                    supplied.push((held, level, index));
                    continue;
                }
                missing.push(Pending {
                    position,
                    held,
                    level,
                    index,
                    demand,
                    current,
                    publish,
                });
                refreshes.remove(request);
            }
        }
        let mut evicted = Vec::new();
        let (demand, prefetch): (Vec<Pending>, Vec<Pending>) =
            missing.into_iter().partition(|held| held.demand);
        let slots = atlas.reserve(time, demand.len(), &mut evicted);
        for brick in evicted {
            if let Some(other) = tables.get_mut(brick.level).and_then(Option::as_mut)
                && other.drop_record(arena, brick.index, brick.time)
            {
                shadow.mark(brick.level, brick.index);
            }
            released.push((brick.time, brick.level, brick.index));
        }
        let prefetch_slots = atlas.reserve_vacant(prefetch.len());
        for (held, slot) in demand
            .iter()
            .zip(&slots)
            .chain(prefetch.iter().zip(&prefetch_slots))
        {
            let (position, packed) = (held.position, &bricks[held.position].1);
            occupancy.reset(*slot);
            for (pack, block) in packed.packs() {
                staged.push((pack, *slot, position));
                occupy(&mut values, occupancy, *slot, pack, block);
            }
            occupancy.flush(upload, *slot);
            atlas.assign(
                *slot,
                ResidentBrick {
                    time: held.held,
                    level: held.level,
                    index: held.index,
                },
                held.demand,
            );
            if held.publish
                && let Some(table) = tables[held.level].as_mut()
            {
                table.set_record(
                    arena,
                    held.index,
                    BrickRecord::new(*slot + 1, held.held, content),
                );
                shadow.mark(held.level, held.index);
            }
            if held.current {
                generation += 1;
            }
            supplied.push((held.held, held.level, held.index));
        }
        residence.set_generation(generation);
        stage(upload, atlas, bricks, staged);
        let uploaded = supplied.len();
        for (held, level, index) in supplied {
            residence.supply(held, level, index);
        }
        for (held, level, index) in released {
            residence.release(pyramid, held, level, index);
        }
        for table in tables.iter_mut().flatten() {
            table.flush(upload, arena);
        }
        uploaded
    }

    pub(super) fn abandon(
        &mut self,
        requests: &[BrickRequest],
        residence: &mut Residence,
        time: u32,
    ) {
        let Self {
            pyramid,
            arena,
            atlas,
            bricks: tables,
            shadow,
            upload,
            ..
        } = self;
        let content = residence.content();
        let mut generation = residence.generation();
        let mut released = Vec::new();
        residence.discard(requests);
        for request in requests {
            let BrickRequest {
                time: held,
                level,
                coord,
                ..
            } = *request;
            if level >= pyramid.level_count()
                || level < arena.first()
                || held >= pyramid.frame_count()
                || held != time
            {
                continue;
            }
            let extent = pyramid.level(level).extent();
            if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
                || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
                || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
            {
                continue;
            }
            let index = BrickGrid::new(extent).index(coord);
            if atlas.slot(held, level, index).is_some() || atlas.is_hollow(held, level, index) {
                released.push((held, level, index));
                continue;
            }
            atlas.assign_hollow(held, level, index);
            if let Some(table) = tables[level]
                .as_mut()
                .filter(|table| table.time() == Some(held))
            {
                table.set_record(arena, index, BrickRecord::hollow(held, content));
                shadow.mark(level, index);
            }
            generation += 1;
            released.push((held, level, index));
        }
        for table in tables.iter_mut().flatten() {
            table.flush(upload, arena);
        }
        residence.set_generation(generation);
        for (held, level, index) in released {
            residence.supply(held, level, index);
        }
    }
}

struct Pending {
    position: usize,
    held: u32,
    level: usize,
    index: usize,
    demand: bool,
    current: bool,
    publish: bool,
}

fn slot_bytes(pyramid: &Pyramid) -> u64 {
    let channels = pyramid.channel_count();
    let source = (0..pack_count(channels))
        .map(|pack| texels(components(channels, pack)) as u64)
        .sum::<u64>()
        * (PADDED_SIDE as u64).pow(3)
        * size_of::<u16>() as u64;
    source + crate::atlas::STRUCTURE_BYTES
}

fn least_budget(pyramid: &Pyramid) -> u64 {
    let coarsest = pyramid.level_count().saturating_sub(1);
    let records = BrickGrid::new(pyramid.level(coarsest).extent()).count() as u64;
    (slot_bytes(pyramid) * MINIMUM_SLOTS).max(records * RECORD_BYTES * RECORD_SHARE)
}

fn occupy(values: &mut [f32], occupancy: &mut Occupancy, slot: u32, pack: usize, block: &Pack) {
    values[0] = block.maximum() / UNIT_MAXIMUM;
    for (value, cell) in values[1..].iter_mut().zip(block.cells()) {
        *value = cell / UNIT_MAXIMUM;
    }
    occupancy.stage(slot, pack, values);
}

fn stage(
    upload: &mut crate::upload::Upload,
    atlas: &Atlas,
    bricks: &[(BrickRequest, PackedBrick)],
    staged: &mut Vec<(usize, u32, usize)>,
) {
    if staged.is_empty() {
        return;
    }
    staged.sort_by_key(|(pack, slot, _)| (*pack, *slot));
    let columns = atlas.slot_grid()[0];
    let mut bytes = Vec::new();
    let mut first = 0;
    while first < staged.len() {
        let (pack, slot, position) = staged[first];
        let block = bricks[position]
            .1
            .pack(pack)
            .expect("a staged brick names one of its packs");
        let mut last = first + 1;
        while last < staged.len() {
            let (other, next, _) = staged[last];
            if other != pack || next != staged[last - 1].1 + 1 || next.is_multiple_of(columns) {
                break;
            }
            last += 1;
        }
        bytes.clear();
        bytes.extend(staged[first..last].iter().map(|(pack, _, position)| {
            let block = bricks[*position]
                .1
                .pack(*pack)
                .expect("a staged brick names one of its packs");
            cast_slice(block.rows())
        }));
        let origin = atlas.origin(slot);
        upload.bricks(
            atlas.texture(pack),
            [origin.x, origin.y, origin.z],
            PADDED_SIDE,
            block.texel_bytes(),
            &bytes,
        );
        first = last;
    }
    staged.clear();
}
