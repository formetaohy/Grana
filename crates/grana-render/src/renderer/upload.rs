use grana_volume::{
    BRICK_SIDE, BrickData, BrickGrid, BrickRequest, CELL_COUNT, CHANNEL_LIMIT, PADDED_SIDE, Pyramid,
};
use wgpu::Queue;

use crate::atlas::{PACK, PACK_LIMIT, ResidentBrick, pack_components, pack_texels};
use crate::brick::BrickRecord;
use crate::occupancy::{OCCUPANCY_WORDS, Occupancy};

use super::Renderer;

pub(super) struct UploadScratch {
    units: Vec<u16>,
    cells: Vec<f32>,
    summaries: Vec<f32>,
    words: Vec<u32>,
}

impl UploadScratch {
    pub fn new() -> Self {
        Self {
            units: Vec::new(),
            cells: vec![0.0; CHANNEL_LIMIT * CELL_COUNT],
            summaries: vec![0.0; PACK_LIMIT * CELL_COUNT],
            words: vec![0; OCCUPANCY_WORDS],
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
            cells,
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
                        let (maximum, summary) = stage(
                            &mut units[..span],
                            cells,
                            &mut summaries[..CELL_COUNT],
                            pyramid,
                            total,
                            data,
                            pack,
                        );
                        atlas.write(queue, pack, slot, &units[..span]);
                        occupy(
                            words,
                            occupancy.as_ref(),
                            queue,
                            slot,
                            pack,
                            maximum,
                            summary,
                        );
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
                let (maximum, _) = stage(
                    &mut units[base..base + step],
                    cells,
                    &mut summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT],
                    pyramid,
                    total,
                    data,
                    pack,
                );
                maxima[pack] = maximum;
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
                    table.set_record(queue, arena, index, BrickRecord::hollow(time, *content));
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
                other.drop_record(queue, arena, brick.index, brick.time);
            }
            let mut base = 0;
            for pack in 0..pack_count {
                let step = voxels * pack_texels(pack_components(total, pack));
                atlas.write(queue, pack, slot, &units[base..base + step]);
                occupy(
                    words,
                    occupancy.as_ref(),
                    queue,
                    slot,
                    pack,
                    maxima[pack],
                    &summaries[pack * CELL_COUNT..(pack + 1) * CELL_COUNT],
                );
                base += step;
            }
            atlas.assign(slot, ResidentBrick { time, level, index }, *pin, demand);
            if publish && let Some(table) = tables[level].as_mut() {
                table.set_record(
                    queue,
                    arena,
                    index,
                    BrickRecord::new(slot + 1, time, *content),
                );
            }
            if current {
                *generation += 1;
            }
            refreshes.remove(request);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stage<'a>(
    units: &mut [u16],
    cells: &mut [f32],
    summary: &'a mut [f32],
    pyramid: &Pyramid,
    channels: usize,
    data: &BrickData,
    pack: usize,
) -> (f32, &'a [f32]) {
    summary.fill(0.0);
    let texels = pack_texels(pack_components(channels, pack));
    let mut maximum = 0.0f32;
    for component in 0..pack_components(channels, pack) {
        let channel = pack * PACK + component;
        let intensity = pyramid.channels()[channel].intensity();
        let channel_cells = &mut cells[channel * CELL_COUNT..(channel + 1) * CELL_COUNT];
        let value =
            data.channels()[channel].pack_units(intensity, units, texels, component, channel_cells);
        maximum = maximum.max(value);
        for (cell, value) in summary.iter_mut().zip(channel_cells.iter()) {
            *cell = cell.max(*value);
        }
    }
    (maximum, summary)
}

fn occupy(
    words: &mut [u32],
    occupancy: Option<&Occupancy>,
    queue: &Queue,
    slot: u32,
    pack: usize,
    maximum: f32,
    summary: &[f32],
) {
    let Some(occupancy) = occupancy else {
        return;
    };
    words[0] = maximum.to_bits();
    for (word, cell) in words[1..].iter_mut().zip(summary) {
        *word = cell.to_bits();
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
