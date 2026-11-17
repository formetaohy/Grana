use bytemuck::cast_slice;
use grana_volume::{
    BRICK_APRON, BRICK_SIDE, BrickData, BrickGrid, BrickRequest, CELL_AXIS, CELL_COUNT, CELL_SIDE,
    PADDED_SIDE, Pyramid, UNIT_MAXIMUM,
};
use rayon::prelude::*;
use wgpu::Queue;

use crate::atlas::{PACK, PACK_LIMIT, ResidentBrick, pack_components, pack_texels, row_pitch};
use crate::brick::BrickRecord;
use crate::occupancy::{CELL_WORDS, Occupancy};

use super::Renderer;

const PADDED: usize = PADDED_SIDE as usize;

const APRON: usize = BRICK_APRON as usize;

const CELL: usize = CELL_SIDE as usize;

const INNER: std::ops::Range<usize> = APRON..APRON + BRICK_SIDE as usize;

pub(super) struct UploadScratch {
    arena: Vec<u16>,
    summaries: Vec<f32>,
    values: Vec<f32>,
}

impl UploadScratch {
    pub fn new() -> Self {
        Self {
            arena: Vec::new(),
            summaries: vec![0.0; PACK_LIMIT * CELL_COUNT],
            values: vec![0.0; CELL_WORDS],
        }
    }
}

struct Layout {
    first: [usize; PACK_LIMIT],
    row: [u32; PACK_LIMIT],
    span: [usize; PACK_LIMIT],
    count: usize,
    elements: usize,
}

impl Layout {
    fn of(channels: usize) -> Self {
        let count = channels.div_ceil(PACK);
        let mut first = [0; PACK_LIMIT];
        let mut row = [0; PACK_LIMIT];
        let mut span = [0; PACK_LIMIT];
        let mut elements = 0;
        for pack in 0..count {
            let pitch = row_pitch(pack_components(channels, pack));
            first[pack] = elements;
            row[pack] = pitch;
            span[pack] = pitch as usize / size_of::<u16>() * PADDED * PADDED;
            elements += span[pack];
        }
        Self {
            first,
            row,
            span,
            count,
            elements,
        }
    }
}

impl Renderer {
    pub fn upload(&mut self, queue: &Queue, bricks: &[(BrickRequest, BrickData)]) {
        let Self {
            bricks: tables,
            atlas,
            arena,
            pyramid,
            channels,
            time: now,
            residence,
            staging,
            scratch,
            occupancy,
            ..
        } = self;
        let (Some(pyramid), Some(atlas), Some(arena)) =
            (pyramid.as_ref(), atlas.as_mut(), arena.as_ref())
        else {
            return;
        };
        let content = residence.content();
        let pin = residence.pin();
        let mut generation = residence.generation();
        let (prefetched, refreshes) = residence.streams();
        let total = *channels;
        let layout = Layout::of(total);
        let UploadScratch {
            arena: units,
            summaries,
            values,
        } = scratch;
        for (request, data) in bricks {
            let BrickRequest {
                time,
                level,
                coord,
                channels: scope,
            } = *request;
            if level >= pyramid.level_count()
                || level < arena.first()
                || time >= pyramid.frame_count()
                || data.channels().len() != total
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
            let voxels = data.channels()[0].voxel_count();
            assert_eq!(
                voxels,
                (PADDED_SIDE as usize).pow(3),
                "an uploaded brick holds a padded cube"
            );
            let packs = pack_mask(scope, total);
            if let Some(slot) = atlas.slot(time, level, index) {
                let refresh = refreshes.remove(request);
                if refresh || !scope.covers(total) {
                    for pack in pack_range(layout.count, packs) {
                        let span = layout.span[pack];
                        if units.len() < span {
                            units.resize(span, 0);
                        }
                        let summary = &mut summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT];
                        let maximum =
                            stage(&mut units[..span], summary, pyramid, total, data, pack);
                        staging.push(pack, slot, layout.row[pack], cast_slice(&units[..span]));
                        occupy(values, occupancy.as_mut(), slot, pack, maximum, summary);
                    }
                    if let Some(occupancy) = occupancy.as_mut() {
                        occupancy.flush(queue, slot);
                    }
                    atlas.written(slot);
                    if time == *now {
                        generation += 1;
                    }
                }
                continue;
            }
            if !scope.covers(total) || atlas.is_hollow(time, level, index) {
                continue;
            }
            if units.len() < layout.elements {
                units.resize(layout.elements, 0);
            }
            let mut maxima = [0.0f32; PACK_LIMIT];
            for pack in 0..layout.count {
                let first = layout.first[pack];
                let span = layout.span[pack];
                let summary = &mut summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT];
                maxima[pack] = stage(
                    &mut units[first..first + span],
                    summary,
                    pyramid,
                    total,
                    data,
                    pack,
                );
            }
            let maximum = maxima[..layout.count].iter().copied().fold(0.0, f32::max);
            let current = time == *now;
            let publish = current
                && tables[level]
                    .as_ref()
                    .is_some_and(|table| table.time() == Some(time));
            if maximum <= 0.0 {
                atlas.assign_hollow(time, level, index);
                if publish && let Some(table) = tables[level].as_mut() {
                    table.set_record(arena, index, BrickRecord::hollow(time, content));
                }
                if current {
                    generation += 1;
                }
                continue;
            }
            let demand = !prefetched.contains(request);
            let Some((slot, evicted)) = atlas.allocate(pin, *now, time, demand) else {
                continue;
            };
            if let Some(brick) = evicted
                && let Some(other) = tables.get_mut(brick.level).and_then(Option::as_mut)
            {
                other.drop_record(arena, brick.index, brick.time);
            }
            if let Some(occupancy) = occupancy.as_mut() {
                occupancy.reset(slot);
            }
            for pack in 0..layout.count {
                let first = layout.first[pack];
                let span = layout.span[pack];
                staging.push(
                    pack,
                    slot,
                    layout.row[pack],
                    cast_slice(&units[first..first + span]),
                );
                occupy(
                    values,
                    occupancy.as_mut(),
                    slot,
                    pack,
                    maxima[pack],
                    &summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT],
                );
            }
            if let Some(occupancy) = occupancy.as_mut() {
                occupancy.flush(queue, slot);
            }
            atlas.assign(slot, ResidentBrick { time, level, index }, pin, demand);
            if publish && let Some(table) = tables[level].as_mut() {
                table.set_record(arena, index, BrickRecord::new(slot + 1, time, content));
            }
            if current {
                generation += 1;
            }
            refreshes.remove(request);
        }
        residence.set_generation(generation);
        for table in tables.iter_mut().flatten() {
            table.flush(queue, arena);
        }
    }
}

fn stage(
    units: &mut [u16],
    summary: &mut [f32],
    pyramid: &Pyramid,
    channels: usize,
    data: &BrickData,
    pack: usize,
) -> f32 {
    let components = pack_components(channels, pack);
    let stride = pack_texels(components);
    let row = units.len() / (PADDED * PADDED);
    assert_eq!(
        units.len(),
        row * PADDED * PADDED,
        "a staged brick holds one padded cube of rows"
    );
    let sources = (0..components)
        .map(|component| {
            data.channels()[pack * PACK + component]
                .units(pyramid.channels()[pack * PACK + component].intensity())
        })
        .collect::<Vec<_>>();
    let (maximum, cells) = units
        .par_chunks_mut(row * PADDED)
        .enumerate()
        .map(|(slice, chunk)| {
            let mut cells = [0.0f32; CELL_COUNT];
            let mut maximum = 0.0f32;
            let range = slice * PADDED * PADDED..(slice + 1) * PADDED * PADDED;
            let plane = INNER
                .contains(&slice)
                .then(|| ((slice - APRON) / CELL) * CELL_AXIS * CELL_AXIS);
            for (component, source) in sources.iter().enumerate() {
                let mut x = 0;
                let mut y = 0;
                source.visit(range.clone(), |fixed| {
                    if fixed > maximum {
                        maximum = fixed;
                    }
                    if let Some(plane) = plane
                        && INNER.contains(&x)
                        && INNER.contains(&y)
                    {
                        let cell = (x - APRON) / CELL + ((y - APRON) / CELL) * CELL_AXIS + plane;
                        if fixed > cells[cell] {
                            cells[cell] = fixed;
                        }
                    }
                    chunk[y * row + x * stride + component] = fixed.round() as u16;
                    x += 1;
                    if x == PADDED {
                        x = 0;
                        y += 1;
                    }
                });
            }
            (maximum, cells)
        })
        .reduce(
            || (0.0f32, [0.0f32; CELL_COUNT]),
            |left, right| {
                let mut cells = left.1;
                for (cell, other) in cells.iter_mut().zip(right.1.iter()) {
                    *cell = cell.max(*other);
                }
                (left.0.max(right.0), cells)
            },
        );
    summary.copy_from_slice(&cells);
    maximum
}

fn occupy(
    values: &mut [f32],
    occupancy: Option<&mut Occupancy>,
    slot: u32,
    pack: usize,
    maximum: f32,
    summary: &[f32],
) {
    let Some(occupancy) = occupancy else {
        return;
    };
    values[0] = maximum / UNIT_MAXIMUM;
    for (value, cell) in values[1..].iter_mut().zip(summary) {
        *value = cell / UNIT_MAXIMUM;
    }
    occupancy.stage(slot, pack, values);
}

fn pack_mask(channels: grana_volume::ChannelSet, count: usize) -> u64 {
    let mut packs = 0u64;
    for pack in 0..count.div_ceil(PACK) {
        let covered = (pack * PACK..(pack + 1) * PACK)
            .all(|channel| channel >= count || channels.contains(channel));
        if covered {
            packs |= 1 << pack;
        }
    }
    packs
}

fn pack_range(pack_count: usize, packs: u64) -> impl Iterator<Item = usize> {
    (0..pack_count).filter(move |pack| packs & (1 << pack) != 0)
}
