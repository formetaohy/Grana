use super::bits::BlockFill;
use super::tile::Tile;
use super::{OUTSIDE, REMOVED};

impl Tile<'_> {
    pub(super) fn mark(&mut self) -> bool {
        let pass = self.pass;
        let anchor = pass.plan.anchor;
        let array = self.array.clone();
        let dims = self.dims;
        let gate = self.gate;
        let solid = self.solid;
        let mask = &mut self.mask;
        let mut low = [usize::MAX; 3];
        let mut high = [0usize; 3];
        let mut occupied = false;
        let shift = [
            array[0].start as i64 - anchor[0],
            array[1].start as i64 - anchor[1],
            array[2].start as i64 - anchor[2],
        ];
        let blocks = [
            shift[0].div_euclid(crate::local::BLOCK),
            shift[1].div_euclid(crate::local::BLOCK),
            shift[2].div_euclid(crate::local::BLOCK),
        ];
        let mut ends = [0i64; 3];
        for axis in 0..3 {
            ends[axis] = (shift[axis] + dims[axis] as i64 - 1).div_euclid(crate::local::BLOCK) + 1;
        }
        for block_z in blocks[2]..ends[2] {
            for block_y in blocks[1]..ends[1] {
                for block_x in blocks[0]..ends[0] {
                    let block = [block_x, block_y, block_z];
                    let mut start = [0usize; 3];
                    let mut stop = [0usize; 3];
                    let mut whole = true;
                    for axis in 0..3 {
                        let first = block[axis] * crate::local::BLOCK - shift[axis];
                        whole &= first >= 0 && first + crate::local::BLOCK <= dims[axis] as i64;
                        start[axis] = (first.max(0) as usize).min(dims[axis]);
                        stop[axis] = ((first + crate::local::BLOCK).min(dims[axis] as i64)
                            as usize)
                            .max(start[axis]);
                    }
                    let fill = if whole {
                        solid.block_fill(block)
                    } else {
                        BlockFill::Mixed
                    };
                    match fill {
                        BlockFill::Empty => continue,
                        BlockFill::Solid => {
                            for z in start[2]..stop[2] {
                                for y in start[1]..stop[1] {
                                    let base = dims[0] * (y + dims[1] * z);
                                    mask[base + start[0]..base + stop[0]].fill(true);
                                }
                            }
                            occupied = true;
                            for axis in 0..3 {
                                low[axis] = low[axis].min(start[axis]);
                                high[axis] = high[axis].max(stop[axis]);
                            }
                        }
                        BlockFill::Mixed => {
                            for z in start[2]..stop[2] {
                                for y in start[1]..stop[1] {
                                    let base = dims[0] * (y + dims[1] * z);
                                    for x in start[0]..stop[0] {
                                        let voxel = [
                                            array[0].start as i64 + x as i64,
                                            array[1].start as i64 + y as i64,
                                            array[2].start as i64 + z as i64,
                                        ];
                                        if gate.admits(voxel) && solid.get(voxel, anchor) {
                                            mask[base + x] = true;
                                            occupied = true;
                                            low[0] = low[0].min(x);
                                            low[1] = low[1].min(y);
                                            low[2] = low[2].min(z);
                                            high[0] = high[0].max(x + 1);
                                            high[1] = high[1].max(y + 1);
                                            high[2] = high[2].max(z + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.space = if occupied {
            let mut space = [0usize; 3];
            for axis in 0..3 {
                space[axis] = low[axis].saturating_sub(1);
            }
            let mut far = [0usize; 3];
            for axis in 0..3 {
                far[axis] = (high[axis] + 1).min(dims[axis]);
            }
            (space, far)
        } else {
            ([0; 3], [0; 3])
        };
        occupied
    }

    pub(super) fn field(&mut self) {
        const WORD: usize = 64;
        const SHIFT: usize = 1;
        let pass = self.pass;
        let anchor = pass.plan.anchor;
        let field_dims = self.field_dims;
        let field_start = self.field_start;
        let origin = [0, 1, 2].map(|axis| (field_start[axis] - self.start[axis]) as usize);
        let gate = self.gate;
        let solid = self.solid;
        let grid = self.grid;
        let detection = self.detection;
        let field = &mut self.field;
        let threshold = pass.plan.threshold;
        let band = self.band.as_ref().expect("a surface tile holds its band");
        let first = origin[0] + SHIFT;
        let last = first + field_dims[0];
        let head = first / WORD;
        let tail = (last - 1) / WORD;
        let keep = (1u64 << (last % WORD).max(1)) - 1;
        for z in 0..field_dims[2] {
            let bitset_z = origin[2] + z + SHIFT;
            let voxel_z = field_start[2] + z as i64;
            for y in 0..field_dims[1] {
                let bitset_y = origin[1] + y + SHIFT;
                let voxel_y = field_start[1] + y as i64;
                let base = field_dims[0] * (y + field_dims[1] * z);
                let mut field_row = grid.row(voxel_y, voxel_z);
                for word in head..=tail {
                    let mut pending = band.needed_word(bitset_y, bitset_z, word);
                    if word == head {
                        pending &= u64::MAX << (first % WORD);
                    }
                    if word == tail && !last.is_multiple_of(WORD) {
                        pending &= keep;
                    }
                    while pending != 0 {
                        let index = word * WORD + pending.trailing_zeros() as usize;
                        pending &= pending - 1;
                        let x = index - SHIFT - origin[0];
                        let voxel = [field_start[0] + x as i64, voxel_y, voxel_z];
                        let slot = base + x;
                        let value = detection.value(voxel);
                        let Some(value) = value else {
                            field[slot] = OUTSIDE;
                            continue;
                        };
                        if !gate.admits(voxel) {
                            field[slot] = OUTSIDE;
                            continue;
                        }
                        let contrast = field_row.contrast(voxel[0], value);
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
}
