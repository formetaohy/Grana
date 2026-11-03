pub(super) const SIDE: usize = crate::local::BLOCK as usize;

const WORD: usize = 64;

pub(super) struct Bits {
    dims: [usize; 3],
    rows: usize,
    words: Vec<u64>,
}

impl Bits {
    pub(super) fn new(dims: [usize; 3]) -> Self {
        let rows = dims[0].div_ceil(WORD);
        Self {
            dims,
            rows,
            words: vec![0; rows * dims[1] * dims[2]],
        }
    }

    pub(super) fn dims(&self) -> [usize; 3] {
        self.dims
    }

    pub(super) fn get(&self, x: usize, y: usize, z: usize) -> bool {
        if x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return false;
        }
        self.words[self.rows * (y + self.dims[1] * z) + x / WORD] & (1 << (x % WORD)) != 0
    }

    pub(super) fn set(&mut self, x: usize, y: usize, z: usize) {
        self.words[self.rows * (y + self.dims[1] * z) + x / WORD] |= 1 << (x % WORD);
    }

    pub(super) fn write_row(&mut self, x0: usize, y: usize, z: usize, bits: &[u64], length: usize) {
        assert!(x0 + length <= self.dims[0], "a bit row fits its array");
        assert_eq!(
            bits.len(),
            length.div_ceil(WORD),
            "a bit row holds its words"
        );
        let row = self.rows * (y + self.dims[1] * z);
        let base = row + x0 / WORD;
        let offset = x0 % WORD;
        if offset == 0 {
            for (slot, value) in bits.iter().enumerate() {
                self.words[base + slot] |= *value;
            }
            return;
        }
        let end = row + self.rows;
        for (slot, value) in bits.iter().enumerate() {
            self.words[base + slot] |= value << offset;
            if base + slot + 1 < end {
                self.words[base + slot + 1] |= value >> (WORD - offset);
            }
        }
    }
}
pub(super) struct Grid {
    dims: [usize; 3],
    blocks: [usize; 3],
    shell: [usize; 3],
    occupied: Vec<bool>,
    outside: Vec<bool>,
    voxel: Bits,
}

impl Grid {
    pub(super) fn new(
        foreground: &Bits,
        admitted: Option<&Bits>,
        occupied: &[bool],
        blocks: [usize; 3],
        dims: [usize; 3],
    ) -> Self {
        assert_eq!(dims, foreground.dims(), "the grid matches its mask");
        assert_eq!(
            occupied.len(),
            blocks[0] * blocks[1] * blocks[2],
            "every block records occupancy"
        );
        let shell = [blocks[0] + 2, blocks[1] + 2, blocks[2] + 2];
        let mut outside = vec![false; shell[0] * shell[1] * shell[2]];
        let mut stack = Vec::new();
        for z in 0..shell[2] {
            for y in 0..shell[1] {
                for x in 0..shell[0] {
                    let border = x == 0
                        || y == 0
                        || z == 0
                        || x + 1 == shell[0]
                        || y + 1 == shell[1]
                        || z + 1 == shell[2];
                    if border {
                        stack.push([x, y, z]);
                    }
                }
            }
        }
        let index = |block: [usize; 3]| block[0] + shell[0] * (block[1] + shell[1] * block[2]);
        while let Some(block) = stack.pop() {
            let slot = index(block);
            if outside[slot] {
                continue;
            }
            if block[0] >= 1
                && block[1] >= 1
                && block[2] >= 1
                && block[0] <= blocks[0]
                && block[1] <= blocks[1]
                && block[2] <= blocks[2]
            {
                let interior = [block[0] - 1, block[1] - 1, block[2] - 1];
                let interior = interior[0] + blocks[0] * (interior[1] + blocks[1] * interior[2]);
                if occupied[interior] {
                    continue;
                }
            }
            outside[slot] = true;
            for axis in 0..3 {
                if block[axis] > 0 {
                    let mut neighbour = block;
                    neighbour[axis] -= 1;
                    stack.push(neighbour);
                }
                if block[axis] + 1 < shell[axis] {
                    let mut neighbour = block;
                    neighbour[axis] += 1;
                    stack.push(neighbour);
                }
            }
        }
        let mut grid = Self {
            dims,
            blocks,
            shell,
            occupied: occupied.to_vec(),
            outside,
            voxel: Bits::new(dims),
        };
        grid.flood(foreground, admitted);
        grid
    }

    pub(super) fn is_solid(
        &self,
        foreground: &Bits,
        admitted: Option<&Bits>,
        x: usize,
        y: usize,
        z: usize,
    ) -> bool {
        if x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return false;
        }
        if admitted.is_some_and(|admitted| !admitted.get(x, y, z)) {
            return false;
        }
        if foreground.get(x, y, z) {
            return true;
        }
        let block = [x / SIDE, y / SIDE, z / SIDE];
        !self.outside
            [self.shell[0] * (1 + block[1] + self.shell[1] * (1 + block[2])) + 1 + block[0]]
            && !self.voxel.get(x, y, z)
    }

    fn barrier(&self, block: [usize; 3]) -> bool {
        block[0] < self.blocks[0]
            && block[1] < self.blocks[1]
            && block[2] < self.blocks[2]
            && self.occupied[block[0] + self.blocks[0] * (block[1] + self.blocks[1] * block[2])]
    }

    fn block_outside(&self, block: [usize; 3]) -> bool {
        self.outside[self.shell[0] * (1 + block[1] + self.shell[1] * (1 + block[2])) + 1 + block[0]]
    }

    fn neighbour_outside(&self, block: [usize; 3], axis: usize, offset: i64) -> bool {
        let mut index = [1 + block[0], 1 + block[1], 1 + block[2]];
        let value = index[axis] as i64 + offset;
        index[axis] = value.clamp(0, self.shell[axis] as i64 - 1) as usize;
        self.outside[index[0] + self.shell[0] * (index[1] + self.shell[1] * index[2])]
    }

    fn free(
        &self,
        foreground: &Bits,
        admitted: Option<&Bits>,
        x: usize,
        y: usize,
        z: usize,
    ) -> bool {
        if foreground.get(x, y, z) || self.voxel.get(x, y, z) {
            return false;
        }
        let block = [x / SIDE, y / SIDE, z / SIDE];
        if !self.barrier(block) || self.block_outside(block) {
            return false;
        }
        admitted.is_none_or(|admitted| admitted.get(x, y, z))
    }

    fn flood(&mut self, foreground: &Bits, admitted: Option<&Bits>) {
        let dims = self.dims;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in [0, dims[0] - 1] {
                    if self.free(foreground, admitted, x, y, z) {
                        self.fill(foreground, admitted, vec![[x, y, z]]);
                    }
                }
            }
        }
        for z in 0..dims[2] {
            for x in 0..dims[0] {
                for y in [0, dims[1] - 1] {
                    if self.free(foreground, admitted, x, y, z) {
                        self.fill(foreground, admitted, vec![[x, y, z]]);
                    }
                }
            }
        }
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                for z in [0, dims[2] - 1] {
                    if self.free(foreground, admitted, x, y, z) {
                        self.fill(foreground, admitted, vec![[x, y, z]]);
                    }
                }
            }
        }
        for z in 0..self.blocks[2] {
            for y in 0..self.blocks[1] {
                for x in 0..self.blocks[0] {
                    let block = [x, y, z];
                    if !self.barrier(block) || self.block_outside(block) {
                        continue;
                    }
                    for axis in 0..3 {
                        for (offset, side) in [(-1i64, 0usize), (1, SIDE - 1)] {
                            if !self.neighbour_outside(block, axis, offset) {
                                continue;
                            }
                            let seeds = self.face_seeds(block, axis, side);
                            self.fill(foreground, admitted, seeds);
                        }
                    }
                }
            }
        }
    }

    fn face_seeds(&self, block: [usize; 3], axis: usize, side: usize) -> Vec<[usize; 3]> {
        let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut seeds = Vec::with_capacity(SIDE * SIDE);
        for second_position in 0..SIDE {
            for first_position in 0..SIDE {
                let mut voxel = [0usize; 3];
                voxel[axis] = block[axis] * SIDE + side;
                voxel[first] = block[first] * SIDE + first_position;
                voxel[second] = block[second] * SIDE + second_position;
                if (0..3).any(|slot| voxel[slot] >= self.dims[slot]) {
                    continue;
                }
                seeds.push(voxel);
            }
        }
        seeds
    }

    fn fill(&mut self, foreground: &Bits, admitted: Option<&Bits>, seeds: Vec<[usize; 3]>) {
        let dims = self.dims;
        let mut stack = seeds;
        while let Some(seed) = stack.pop() {
            let [x, y, z] = seed;
            if !self.free(foreground, admitted, x, y, z) {
                continue;
            }
            let mut left = x;
            while left > 0 && self.free(foreground, admitted, left - 1, y, z) {
                left -= 1;
            }
            let mut right = x;
            while right + 1 < dims[0] && self.free(foreground, admitted, right + 1, y, z) {
                right += 1;
            }
            for position in left..=right {
                self.voxel.set(position, y, z);
            }
            for axis in [1usize, 2] {
                for offset in [-1i64, 1] {
                    let value = [x, y, z][axis] as i64 + offset;
                    if value < 0 || value >= dims[axis] as i64 {
                        continue;
                    }
                    let mut neighbour = [x, y, z];
                    neighbour[axis] = value as usize;
                    let mut position = left;
                    while position <= right {
                        if self.free(foreground, admitted, position, neighbour[1], neighbour[2]) {
                            let mut start = position;
                            while start > 0
                                && self.free(
                                    foreground,
                                    admitted,
                                    start - 1,
                                    neighbour[1],
                                    neighbour[2],
                                )
                            {
                                start -= 1;
                            }
                            stack.push([start, neighbour[1], neighbour[2]]);
                            while position <= right
                                && self.free(
                                    foreground,
                                    admitted,
                                    position,
                                    neighbour[1],
                                    neighbour[2],
                                )
                            {
                                position += 1;
                            }
                        } else {
                            position += 1;
                        }
                    }
                }
            }
        }
    }
}
