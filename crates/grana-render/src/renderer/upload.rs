use grana_volume::{
    BRICK_SIDE, BrickData, BrickGrid, BrickRequest, CELL_COUNT, CHANNEL_LIMIT, PADDED_SIDE,
};
use wgpu::Queue;

use crate::atlas::{PACK, ResidentBrick, pack_components, pack_texels};
use crate::brick::BrickRecord;

use super::{Renderer, ensure_brick};

pub(super) struct UploadScratch {
    units: Vec<u16>,
    cells: Vec<f32>,
}

impl UploadScratch {
    pub fn new() -> Self {
        Self {
            units: Vec::new(),
            cells: vec![0.0; CHANNEL_LIMIT * CELL_COUNT],
        }
    }
}

impl Renderer {
    pub fn upload(&mut self, queue: &Queue, bricks: &[(BrickRequest, BrickData)]) -> usize {
        let Self {
            bricks: tables,
            atlas,
            summary,
            arena,
            pyramid,
            channels,
            time: now,
            content,
            pin,
            prefetched,
            staging,
            ..
        } = self;
        let (Some(pyramid), Some(atlas), Some(summary), Some(arena)) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            summary.as_mut(),
            arena.as_ref(),
        ) else {
            return 0;
        };
        let UploadScratch { units, cells } = staging;
        let packs = (*channels).div_ceil(PACK);
        let total = (0..packs)
            .map(|pack| pack_texels(pack_components(*channels, pack)))
            .sum::<usize>();
        let mut accepted = 0;
        for (request, data) in bricks {
            let BrickRequest { time, level, coord } = *request;
            if level >= pyramid.level_count()
                || level < arena.first()
                || time >= pyramid.frame_count()
                || data.channels().len() != *channels
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
            if atlas.slot(time, level, index).is_some() || atlas.is_hollow(time, level, index) {
                accepted += 1;
                continue;
            }
            let voxels = data.channels()[0].voxel_count();
            assert_eq!(
                voxels,
                (PADDED_SIDE as usize).pow(3),
                "an uploaded brick holds a padded cube"
            );
            if units.len() < voxels * total {
                units.resize(voxels * total, 0);
            }
            let mut maxima = [[0.0f32; 4]; CHANNEL_LIMIT / 4];
            let mut base = 0;
            for pack in 0..packs {
                let components = pack_components(*channels, pack);
                let texels = pack_texels(components);
                let span = voxels * texels;
                let packed = &mut units[base..base + span];
                for component in 0..components {
                    let channel = pack * PACK + component;
                    let intensity = pyramid.channels()[channel].intensity();
                    let summary_cells =
                        &mut cells[channel * CELL_COUNT..(channel + 1) * CELL_COUNT];
                    maxima[channel / 4][channel % 4] = data.channels()[channel].pack_units(
                        intensity,
                        &mut packed[..],
                        texels,
                        component,
                        summary_cells,
                    );
                }
                base += span;
            }
            if maxima.iter().flatten().all(|maximum| *maximum <= 0.0) {
                atlas.assign_hollow(time, level, index);
                if let Some(table) = tables[level].as_mut() {
                    table.set_record(queue, arena, index, BrickRecord::hollow(time));
                }
                *content += 1;
                accepted += 1;
                continue;
            }
            let slot_grid = atlas.slot_grid();
            ensure_brick(tables, pyramid.level(level), level, slot_grid, arena);
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
            for pack in 0..packs {
                let span = voxels * pack_texels(pack_components(*channels, pack));
                atlas.write(queue, pack, slot, &units[base..base + span]);
                base += span;
            }
            summary.write(queue, slot, &cells[..*channels * CELL_COUNT]);
            atlas.assign(
                slot,
                ResidentBrick {
                    time,
                    level,
                    index,
                    maxima,
                },
                *pin,
                demand,
            );
            if let Some(table) = tables[level].as_mut() {
                table.set_record(
                    queue,
                    arena,
                    index,
                    BrickRecord::new(slot + 1, time, maxima),
                );
            }
            *content += 1;
            accepted += 1;
        }
        accepted
    }
}
