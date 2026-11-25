use bytemuck::cast_slice;
use grana_volume::{
    BRICK_SIDE, BrickCoord, BrickGrid, BrickRequest, ChannelSet, Damage, PADDED_SIDE, Pack,
    PackedBrick, UNIT_MAXIMUM,
};

use crate::atlas::ResidentBrick;
use crate::brick::BrickRecord;
use crate::occupancy::{CELL_WORDS, Occupancy};
use crate::reserve::Plan;

use super::*;

const APRON: u32 = 1;

pub(super) enum Shortfall {
    Memory,
    Detail(String),
}

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
    pub(super) surface: u64,
    pub(super) summary: Option<signature::SceneSummary>,
}

impl Resources {
    pub(super) fn new(
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        sampler: &Sampler,
        dummy: &TextureView,
        pyramid: &Pyramid,
        plan: &Plan,
    ) -> Result<Self, Shortfall> {
        match grana_gpu::scoped(device, || {
            Self::allocate(device, layouts, pipelines, sampler, dummy, pyramid, plan)
        }) {
            Ok(Ok(resources)) => Ok(resources),
            Ok(Err(detail)) => Err(Shortfall::Detail(detail)),
            Err(wgpu::Error::OutOfMemory { .. }) => Err(Shortfall::Memory),
            Err(error) => Err(Shortfall::Detail(format!(
                "the renderer could not allocate its dataset resources: {error}"
            ))),
        }
    }

    fn allocate(
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        sampler: &Sampler,
        dummy: &TextureView,
        pyramid: &Pyramid,
        plan: &Plan,
    ) -> Result<Self, String> {
        let channels = pyramid.channel_count();
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a dataset holds one to {CHANNEL_LIMIT} channels"
        );
        assert!(
            !pyramid.levels().is_empty(),
            "a pyramid holds at least one level"
        );
        let atlas = Atlas::new(
            device,
            crate::atlas::AtlasBinding {
                layout: &layouts.atlas,
                source: &layouts.source,
                dummy,
                sampler,
            },
            channels,
            plan.slots(),
        );
        let slots = atlas.slots();
        let occupancy = Occupancy::new(device, slots);
        let bake = Bake::new(device, layouts, atlas.structure(), slots, pipelines);
        let arena = BrickArena::new(device, pyramid, plan.record_limit())
            .expect("the record share holds the coarsest level");
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
        let shadow = Shadow::new(device, layouts, pipelines, pyramid);
        let scene = crate::table::SceneTable::new(device, layouts, channels);
        let upload = crate::upload::Upload::new(device);
        let resources = Self {
            pyramid: pyramid.clone(),
            atlas,
            arena,
            bricks: (0..pyramid.level_count()).map(|_| None).collect(),
            occupancy,
            occupancy_group,
            bake,
            baked: BakeQueue::new(),
            upload,
            staged: Vec::new(),
            level_group,
            planner,
            shadow,
            scene,
            channels,
            slot_bytes: plan.slot_bytes(),
            surface: plan.surface(),
            summary: None,
        };
        assert_eq!(
            resources.bytes() + planner::readback_bytes(slots),
            plan.dataset(),
            "the renderer's resources hold exactly the bytes its plan charges"
        );
        Ok(resources)
    }

    pub(super) fn bytes(&self) -> u64 {
        self.atlas.bytes()
            + self.arena.bytes()
            + self.occupancy.bytes()
            + self.bake.bytes()
            + self.planner.bytes()
            + self.shadow.bytes()
            + self.scene.bytes()
            + crate::upload::Upload::bytes()
    }

    pub(super) fn slots(&self) -> usize {
        self.atlas.slots()
    }

    pub(super) fn drop_hollows(&mut self, forgotten: &[u32]) {
        let Self {
            bricks,
            arena,
            upload,
            ..
        } = self;
        forget_hollows(bricks, arena, upload, forgotten);
    }

    pub(super) fn surface_bytes(&self) -> u64 {
        self.surface
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
    ) -> Uploaded {
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
        let mut settled = Vec::new();
        let mut released = Vec::new();
        let mut missing = Vec::new();
        let mut dropped = Vec::new();
        let mut forgotten = Vec::new();
        let total = *channels;
        let mut values = [0.0f32; CELL_WORDS];
        {
            let placement = Placement {
                pyramid,
                arena,
                total,
                time,
            };
            let (prefetched, refreshes) = residence.streams();
            for (position, (request, packed)) in bricks.iter().enumerate() {
                let BrickRequest {
                    time: held,
                    level,
                    channels: scope,
                    ..
                } = *request;
                let Some(index) = placement.index(request, packed) else {
                    prefetched.remove(request);
                    dropped.push(*request);
                    continue;
                };
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
                    settled.push(*request);
                    continue;
                }
                if !scope.covers(total) || atlas.is_hollow(held, level, index) {
                    dropped.push(*request);
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
                    forgotten.extend(atlas.assign_hollow(held, level, index));
                    if publish {
                        if let Some(table) = tables[level].as_mut() {
                            table.set_record(arena, index, BrickRecord::hollow(held, content));
                        }
                        shadow.mark(level, index);
                    }
                    if current {
                        generation += 1;
                    }
                    settled.push(*request);
                    continue;
                }
                missing.push(Pending {
                    request: *request,
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
        let placements = demand
            .iter()
            .zip(slots.iter())
            .chain(prefetch.iter().zip(prefetch_slots.iter()));
        for (pending, slot) in placements {
            let (position, packed) = (pending.position, &bricks[pending.position].1);
            occupancy.reset(*slot);
            for (pack, block) in packed.packs() {
                staged.push((pack, *slot, position));
                occupy(&mut values, occupancy, *slot, pack, block);
            }
            occupancy.flush(upload, *slot);
            atlas.assign(
                *slot,
                ResidentBrick {
                    time: pending.held,
                    level: pending.level,
                    index: pending.index,
                },
                pending.demand,
            );
            if pending.publish
                && let Some(table) = tables[pending.level].as_mut()
            {
                table.set_record(
                    arena,
                    pending.index,
                    BrickRecord::new(*slot + 1, pending.held, content),
                );
                shadow.mark(pending.level, pending.index);
            }
            if pending.current {
                generation += 1;
            }
            settled.push(pending.request);
        }
        let denied = demand
            .iter()
            .skip(slots.len())
            .map(|pending| pending.request)
            .collect::<Vec<_>>();
        dropped.extend(
            prefetch
                .iter()
                .skip(prefetch_slots.len())
                .map(|pending| pending.request),
        );
        residence.set_generation(generation);
        stage(upload, atlas, bricks, staged);
        let uploaded = settled.len();
        for request in settled {
            residence.supply(request);
        }
        for (held, level, index) in released {
            residence.release(pyramid, held, level, index);
        }
        forget_hollows(tables, arena, upload, &forgotten);
        for table in tables.iter_mut().flatten() {
            table.flush(upload, arena);
        }
        Uploaded {
            written: uploaded,
            denied,
            dropped,
        }
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
            ..
        } = self;
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
            if atlas.slot(held, level, index).is_some() {
                continue;
            }
            atlas.assign_unavailable(held, level, index);
            residence.abandoned(*request);
        }
    }

    pub(super) fn retire(&mut self, time: u32, base: usize, count: usize) {
        self.atlas.retire(time, base, count);
    }

    pub(super) fn forget_unavailable(&mut self, requests: &[BrickRequest]) {
        let Self {
            pyramid,
            arena,
            atlas,
            ..
        } = self;
        for request in requests {
            let BrickRequest {
                time, level, coord, ..
            } = *request;
            if level >= pyramid.level_count() || level < arena.first() {
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
            atlas.retry(time, level, index);
        }
    }

    pub(super) fn forget_unavailable_time(&mut self, time: u32) {
        self.atlas.retry_time(time);
    }
}

struct Pending {
    request: BrickRequest,
    position: usize,
    held: u32,
    level: usize,
    index: usize,
    demand: bool,
    current: bool,
    publish: bool,
}

pub(super) struct Uploaded {
    pub(super) written: usize,
    pub(super) denied: Vec<BrickRequest>,
    pub(super) dropped: Vec<BrickRequest>,
}

struct Placement<'a> {
    pyramid: &'a Pyramid,
    arena: &'a BrickArena,
    total: usize,
    time: u32,
}

impl Placement<'_> {
    fn index(&self, request: &BrickRequest, packed: &PackedBrick) -> Option<usize> {
        let BrickRequest {
            time, level, coord, ..
        } = *request;
        if time < self.time
            || level >= self.pyramid.level_count()
            || level < self.arena.first()
            || time >= self.pyramid.frame_count()
            || packed.channels() != self.total
        {
            return None;
        }
        let extent = self.pyramid.level(level).extent();
        if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
            || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
            || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
        {
            return None;
        }
        Some(BrickGrid::new(extent).index(coord))
    }
}

fn forget_hollows(
    tables: &mut [Option<BrickTable>],
    arena: &BrickArena,
    upload: &mut crate::upload::Upload,
    forgotten: &[u32],
) {
    for time in forgotten {
        for table in tables.iter_mut().flatten() {
            if table.drop_hollows(arena, *time) {
                table.flush(upload, arena);
            }
        }
    }
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
