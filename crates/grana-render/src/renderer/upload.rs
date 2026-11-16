use grana_volume::{
    BRICK_APRON, BRICK_SIDE, BrickData, BrickGrid, BrickRequest, CELL_AXIS, CELL_COUNT, CELL_SIDE,
    PADDED_SIDE, Pyramid, UNIT_MAXIMUM,
};
use rayon::prelude::*;
use wgpu::Queue;

use crate::atlas::{PACK, PACK_LIMIT, ResidentBrick, pack_components, pack_texels};
use crate::brick::BrickRecord;
use crate::occupancy::{CELL_WORDS, Occupancy};

use super::Renderer;

const PADDED: usize = PADDED_SIDE as usize;

const APRON: usize = BRICK_APRON as usize;

const CELL: usize = CELL_SIDE as usize;

const SLICE: usize = PADDED * PADDED;

const INNER: std::ops::Range<usize> = APRON..APRON + BRICK_SIDE as usize;

pub(super) struct UploadScratch {
    units: Vec<u16>,
    summaries: Vec<f32>,
    words: Vec<u32>,
}

impl UploadScratch {
    pub fn new() -> Self {
        Self {
            units: Vec::new(),
            summaries: vec![0.0; PACK_LIMIT * CELL_COUNT],
            words: vec![0; CELL_WORDS],
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
            content,
            generation,
            pin,
            prefetched,
            staging,
            occupancy,
            refreshes,
            ..
        } = self;
        let (Some(pyramid), Some(atlas), Some(arena)) =
            (pyramid.as_ref(), atlas.as_mut(), arena.as_ref())
        else {
            return;
        };
        let total = *channels;
        let pack_count = total.div_ceil(PACK);
        let texels = (0..pack_count)
            .map(|pack| pack_texels(pack_components(total, pack)))
            .sum::<usize>();
        let UploadScratch {
            units,
            summaries,
            words,
        } = staging;
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
                    for pack in pack_range(pack_count, packs) {
                        let span = pack_texels(pack_components(total, pack)) * voxels;
                        if units.len() < span {
                            units.resize(span, 0);
                        }
                        let summary = &mut summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT];
                        let maximum =
                            stage(&mut units[..span], summary, pyramid, total, data, pack);
                        atlas.write(queue, pack, slot, &units[..span]);
                        occupy(
                            words,
                            occupancy.as_mut(),
                            queue,
                            slot,
                            pack,
                            maximum,
                            summary,
                        );
                    }
                    if let Some(occupancy) = occupancy.as_ref() {
                        occupancy.seal(queue, slot);
                    }
                    atlas.written(slot);
                    if time == *now {
                        *generation += 1;
                    }
                }
                continue;
            }
            if !scope.covers(total) || atlas.is_hollow(time, level, index) {
                continue;
            }
            let span = voxels * texels;
            if units.len() < span {
                units.resize(span, 0);
            }
            let mut maxima = [0.0f32; PACK_LIMIT];
            let mut base = 0;
            for pack in 0..pack_count {
                let step = voxels * pack_texels(pack_components(total, pack));
                maxima[pack] = stage(
                    &mut units[base..base + step],
                    &mut summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT],
                    pyramid,
                    total,
                    data,
                    pack,
                );
                base += step;
            }
            let maximum = maxima[..pack_count].iter().copied().fold(0.0, f32::max);
            let current = time == *now;
            let publish = current
                && tables[level]
                    .as_ref()
                    .is_some_and(|table| table.time() == Some(time));
            if maximum <= 0.0 {
                atlas.assign_hollow(time, level, index);
                if publish && let Some(table) = tables[level].as_mut() {
                    table.set_record(arena, index, BrickRecord::hollow(time, *content));
                }
                if current {
                    *generation += 1;
                }
                continue;
            }
            let demand = !prefetched.contains(request);
            let Some((slot, evicted)) = atlas.allocate(*pin, *now, time, demand) else {
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
            let mut base = 0;
            for pack in 0..pack_count {
                let step = voxels * pack_texels(pack_components(total, pack));
                atlas.write(queue, pack, slot, &units[base..base + step]);
                occupy(
                    words,
                    occupancy.as_mut(),
                    queue,
                    slot,
                    pack,
                    maxima[pack],
                    &summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT],
                );
                base += step;
            }
            if let Some(occupancy) = occupancy.as_ref() {
                occupancy.seal(queue, slot);
            }
            atlas.assign(slot, ResidentBrick { time, level, index }, *pin, demand);
            if publish && let Some(table) = tables[level].as_mut() {
                table.set_record(arena, index, BrickRecord::new(slot + 1, time, *content));
            }
            if current {
                *generation += 1;
            }
            refreshes.remove(request);
        }
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
    let sources = (0..components)
        .map(|component| {
            data.channels()[pack * PACK + component]
                .units(pyramid.channels()[pack * PACK + component].intensity())
        })
        .collect::<Vec<_>>();
    let (maximum, cells) = units
        .par_chunks_mut(SLICE * stride)
        .enumerate()
        .map(|(slice, chunk)| {
            let mut cells = [0.0f32; CELL_COUNT];
            let mut maximum = 0.0f32;
            let range = slice * SLICE..(slice + 1) * SLICE;
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
                    chunk[(y * PADDED + x) * stride + component] = fixed.round() as u16;
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
    words: &mut [u32],
    occupancy: Option<&mut Occupancy>,
    queue: &Queue,
    slot: u32,
    pack: usize,
    maximum: f32,
    summary: &[f32],
) {
    let Some(occupancy) = occupancy else {
        return;
    };
    words[0] = (maximum / UNIT_MAXIMUM).to_bits();
    for (word, cell) in words[1..].iter_mut().zip(summary) {
        *word = (cell / UNIT_MAXIMUM).to_bits();
    }
    occupancy.write(queue, slot, pack, words);
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
