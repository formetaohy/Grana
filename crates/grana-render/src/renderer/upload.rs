use bytemuck::cast_slice;
use grana_volume::{
    BRICK_SIDE, BrickGrid, BrickRequest, PADDED_SIDE, Pack, PackedBrick, UNIT_MAXIMUM,
};

use crate::atlas::ResidentBrick;
use crate::brick::BrickRecord;
use crate::occupancy::{CELL_WORDS, Occupancy};
use crate::upload::Upload;

use super::Renderer;

impl Renderer {
    pub fn upload(&mut self, bricks: &[(BrickRequest, PackedBrick)]) {
        let Self {
            bricks: tables,
            atlas,
            arena,
            pyramid,
            channels,
            time: now,
            residence,
            upload,
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
        let mut values = [0.0f32; CELL_WORDS];
        for (request, packed) in bricks {
            let BrickRequest {
                time,
                level,
                coord,
                channels: scope,
            } = *request;
            if level >= pyramid.level_count()
                || level < arena.first()
                || time >= pyramid.frame_count()
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
            if let Some(slot) = atlas.slot(time, level, index) {
                let refresh = refreshes.remove(request);
                if refresh || !scope.covers(total) {
                    for (pack, held) in packed.packs() {
                        push_texels(upload, atlas, pack, slot, held);
                        occupy(&mut values, occupancy.as_mut(), slot, pack, held);
                    }
                    if let Some(occupancy) = occupancy.as_mut() {
                        occupancy.flush(upload, slot);
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
            let maximum = packed
                .packs()
                .map(|(_, held)| held.maximum())
                .fold(0.0, f32::max);
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
            for (pack, held) in packed.packs() {
                push_texels(upload, atlas, pack, slot, held);
                occupy(&mut values, occupancy.as_mut(), slot, pack, held);
            }
            if let Some(occupancy) = occupancy.as_mut() {
                occupancy.flush(upload, slot);
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
            table.flush(upload, arena);
        }
    }
}

fn occupy(
    values: &mut [f32],
    occupancy: Option<&mut Occupancy>,
    slot: u32,
    pack: usize,
    held: &Pack,
) {
    let Some(occupancy) = occupancy else {
        return;
    };
    values[0] = held.maximum() / UNIT_MAXIMUM;
    for (value, cell) in values[1..].iter_mut().zip(held.cells()) {
        *value = cell / UNIT_MAXIMUM;
    }
    occupancy.stage(slot, pack, values);
}

fn push_texels(
    upload: &mut Upload,
    atlas: &crate::atlas::Atlas,
    pack: usize,
    slot: u32,
    held: &Pack,
) {
    let origin = atlas.origin(slot);
    upload.texture(
        atlas.texture(pack),
        [origin.x, origin.y, origin.z],
        [PADDED_SIDE, PADDED_SIDE, PADDED_SIDE],
        held.row_pitch(),
        PADDED_SIDE,
        cast_slice(held.rows()),
    );
}
