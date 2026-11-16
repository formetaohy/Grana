use grana_volume::{BrickCoord, BrickGrid, BrickRequest, ChannelSet, Damage};
use wgpu::Queue;

use crate::atlas::PACK;

use super::Renderer;

const APRON: u32 = 1;

impl Renderer {
    pub fn damage(&mut self, queue: &Queue, damages: &[Damage]) {
        let mut scopes = Vec::with_capacity(damages.len());
        for damage in damages {
            let channels = self.scope(damage.channels());
            if !channels.is_empty() {
                scopes.push((damage.region(), channels));
            }
        }
        if scopes.is_empty() {
            return;
        }
        for target in self.targets.iter_mut() {
            target.shot = None;
            target.holds = false;
        }
        self.prefetched.clear();
        self.prefetch_issued.clear();
        let Self {
            pyramid,
            arena,
            atlas,
            bricks,
            refreshes,
            time,
            ..
        } = self;
        let (Some(pyramid), Some(arena), Some(atlas)) =
            (pyramid.as_ref(), arena.as_ref(), atlas.as_mut())
        else {
            return;
        };
        let mut released = Vec::new();
        let mut resident = Vec::new();
        let mut dropped = Vec::new();
        let evicted = atlas.evict_other_frames(*time);
        for brick in evicted {
            if let Some(table) = bricks[brick.level].as_mut() {
                table.drop_record(arena, brick.index, brick.time);
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
                                    .insert(BrickRequest::of(brick.time, level, coord, channels));
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
            if let Some(table) = bricks[level].as_mut() {
                table.drop_record(arena, index, brick_time);
            }
        }
        for table in bricks.iter_mut().flatten() {
            table.flush(queue, arena);
        }
    }

    fn scope(&self, channels: ChannelSet) -> ChannelSet {
        let mut scope = ChannelSet::NONE;
        for channel in channels.iter().filter(|channel| *channel < self.channels) {
            for member in channel / PACK * PACK..(channel / PACK + 1) * PACK {
                if member < self.channels {
                    scope = scope.with(member);
                }
            }
        }
        scope
    }
}
