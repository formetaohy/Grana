use super::tile::Tile;
use super::{OUTSIDE, REMOVED};

impl Tile<'_> {
    pub(super) fn prepare(&mut self) {
        let Self {
            pass,
            solid,
            grid,
            gate,
            array,
            dims,
            raw,
            field,
            mask,
            ..
        } = self;
        let threshold = pass.plan.threshold;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                let base = dims[0] * (y + dims[1] * z);
                for x in 0..dims[0] {
                    let voxel = [
                        array[0].start as i64 + x as i64,
                        array[1].start as i64 + y as i64,
                        array[2].start as i64 + z as i64,
                    ];
                    let slot = base + x;
                    if !gate.admits(voxel) {
                        field[slot] = OUTSIDE;
                        mask[slot] = false;
                        continue;
                    }
                    let contrast = grid.contrast(voxel, raw[slot]);
                    let inside = solid.get(voxel, pass.plan.anchor);
                    mask[slot] = inside;
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
