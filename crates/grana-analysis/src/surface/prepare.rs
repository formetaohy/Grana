use super::tile::Tile;
use super::{OUTSIDE, REMOVED};

impl Tile<'_> {
    pub(super) fn mark(&mut self) -> bool {
        let Self {
            pass,
            solid,
            gate,
            array,
            dims,
            mask,
            ..
        } = self;
        let anchor = pass.plan.anchor;
        let mut occupied = false;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                let base = dims[0] * (y + dims[1] * z);
                for x in 0..dims[0] {
                    let voxel = [
                        array[0].start as i64 + x as i64,
                        array[1].start as i64 + y as i64,
                        array[2].start as i64 + z as i64,
                    ];
                    let inside = gate.admits(voxel) && solid.get(voxel, anchor);
                    mask[base + x] = inside;
                    occupied |= inside;
                }
            }
        }
        occupied
    }

    pub(super) fn field(&mut self) {
        let Self {
            pass,
            solid,
            grid,
            gate,
            value_dims,
            value_start,
            smoothed,
            field,
            ..
        } = self;
        let threshold = pass.plan.threshold;
        let anchor = pass.plan.anchor;
        for z in 0..value_dims[2] {
            for y in 0..value_dims[1] {
                let base = value_dims[0] * (y + value_dims[1] * z);
                for x in 0..value_dims[0] {
                    let voxel = [
                        value_start[0] + x as i64,
                        value_start[1] + y as i64,
                        value_start[2] + z as i64,
                    ];
                    let slot = base + x;
                    if !gate.admits(voxel) {
                        field[slot] = OUTSIDE;
                        continue;
                    }
                    let contrast = grid.contrast(voxel, smoothed[slot]);
                    let inside = solid.get(voxel, anchor);
                    field[slot] = match (inside, contrast >= threshold) {
                        (true, true) => contrast,
                        (true, false) => threshold,
                        (false, true) => threshold - REMOVED,
                        (false, false) => contrast,
                    };
                }
            }
        }
    }
}
