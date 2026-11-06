use crate::local::BlockField;

pub(super) const SIDE: usize = crate::local::BLOCK as usize;

const WORD: usize = 64;

fn block_of(voxel: [usize; 3]) -> [i64; 3] {
    [
        (voxel[0] / SIDE) as i64,
        (voxel[1] / SIDE) as i64,
        (voxel[2] / SIDE) as i64,
    ]
}

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

    pub(super) fn rows(&self) -> usize {
        self.rows
    }

    pub(super) fn word(&self, y: usize, z: usize, word: usize) -> u64 {
        self.words[self.rows * (y + self.dims[1] * z) + word]
    }

    pub(super) fn set_word(&mut self, y: usize, z: usize, word: usize, value: u64) {
        self.words[self.rows * (y + self.dims[1] * z) + word] = value;
    }

    pub(super) fn line(&self, y: usize, z: usize) -> &[u64] {
        let base = self.rows * (y + self.dims[1] * z);
        &self.words[base..base + self.rows]
    }

    pub(super) fn words(&self) -> &[u64] {
        &self.words
    }

    pub(super) fn from_words(dims: [usize; 3], words: Vec<u64>) -> Self {
        let rows = dims[0].div_ceil(WORD);
        assert_eq!(
            words.len(),
            rows * dims[1] * dims[2],
            "a bit grid holds the words of its dims"
        );
        Self { dims, rows, words }
    }

    pub(super) fn get(&self, x: usize, y: usize, z: usize) -> bool {
        if x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return false;
        }
        self.words[self.rows * (y + self.dims[1] * z) + x / WORD] & (1 << (x % WORD)) != 0
    }

    pub(super) fn any(&self, block: [usize; 3]) -> bool {
        let Some((word, mask, first, last)) = self.span(block) else {
            return false;
        };
        (first[1]..last[1]).any(|y| {
            (first[2]..last[2]).any(|z| {
                let row = self.rows * (y + self.dims[1] * z) + word;
                self.words[row] & mask != 0
            })
        })
    }

    pub(super) fn all(&self, block: [usize; 3]) -> bool {
        let Some((word, mask, first, last)) = self.span(block) else {
            return false;
        };
        (first[1]..last[1]).all(|y| {
            (first[2]..last[2]).all(|z| {
                let row = self.rows * (y + self.dims[1] * z) + word;
                self.words[row] & mask == mask
            })
        })
    }

    fn span(&self, block: [usize; 3]) -> Option<(usize, u64, [usize; 3], [usize; 3])> {
        let mut first = [0usize; 3];
        let mut last = [0usize; 3];
        for axis in 0..3 {
            let start = block[axis] * SIDE;
            if start + SIDE > self.dims[axis] {
                return None;
            }
            first[axis] = start;
            last[axis] = start + SIDE;
        }
        let word = first[0] / WORD;
        let offset = first[0] % WORD;
        let mask = ((1u64 << SIDE) - 1) << offset;
        Some((word, mask, first, last))
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

    pub(super) fn or_word(&mut self, x: usize, y: usize, z: usize, value: u64, length: usize) {
        if value == 0 {
            return;
        }
        let row = self.rows * (y + self.dims[1] * z);
        let word = x / WORD;
        let offset = x % WORD;
        self.words[row + word] |= value << offset;
        if offset + length > WORD && word + 1 < self.rows {
            self.words[row + word + 1] |= value >> (WORD - offset);
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
    growth: Bits,
}

pub(super) struct MaskInputs<'a> {
    pub(super) foreground: &'a Bits,
    pub(super) admitted: Option<&'a Bits>,
    pub(super) raised: &'a Bits,
    pub(super) field: &'a BlockField,
}

impl Grid {
    pub(super) fn new(
        mask: &MaskInputs<'_>,
        occupied: &[bool],
        blocks: [usize; 3],
        dims: [usize; 3],
        workers: usize,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Self {
        let foreground = mask.foreground;
        let admitted = mask.admitted;
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
            growth: Bits::new(dims),
        };
        grid.flood(foreground, admitted, workers, cancel);
        grid.grow(mask);
        grid
    }
}

pub(super) enum BlockFill {
    Empty,
    Solid,
    Mixed,
}

impl Grid {
    pub(super) fn block_fill(
        &self,
        foreground: &Bits,
        admitted: Option<&Bits>,
        block: [i64; 3],
    ) -> BlockFill {
        let mut index = [0usize; 3];
        for axis in 0..3 {
            let start = block[axis] * SIDE as i64;
            if start < 0 || start + SIDE as i64 > self.dims[axis] as i64 {
                return BlockFill::Mixed;
            }
            index[axis] = start as usize / SIDE;
        }
        if let Some(admitted) = admitted {
            if !admitted.any(index) {
                return BlockFill::Empty;
            }
            if !admitted.all(index) {
                return BlockFill::Mixed;
            }
        }
        if foreground.any(index) || self.growth.any(index) {
            return BlockFill::Mixed;
        }
        if self.outside
            [self.shell[0] * (1 + index[1] + self.shell[1] * (1 + index[2])) + 1 + index[0]]
        {
            return BlockFill::Empty;
        }
        if self.voxel.any(index) {
            return BlockFill::Mixed;
        }
        BlockFill::Solid
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
        if foreground.get(x, y, z) || self.growth.get(x, y, z) {
            return true;
        }
        let block = [x / SIDE, y / SIDE, z / SIDE];
        !self.outside
            [self.shell[0] * (1 + block[1] + self.shell[1] * (1 + block[2])) + 1 + block[0]]
            && !self.voxel.get(x, y, z)
    }

    fn grow(&mut self, mask: &MaskInputs<'_>) {
        let dims = self.dims;
        let rows = mask.foreground.rows();
        let mut queue = std::collections::VecDeque::new();
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for word in 0..rows {
                    let mut bits = mask.foreground.word(y, z, word);
                    while bits != 0 {
                        let x = word * WORD + bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        if x >= dims[0] {
                            continue;
                        }
                        let reference = mask.field.at(block_of([x, y, z])).1;
                        for axis in 0..3 {
                            for step in [-1i64, 1] {
                                let reached = [x, y, z][axis] as i64 + step;
                                if reached < 0 || reached >= dims[axis] as i64 {
                                    continue;
                                }
                                let mut neighbour = [x, y, z];
                                neighbour[axis] = reached as usize;
                                self.enter(&mut queue, mask, neighbour, reference);
                            }
                        }
                    }
                }
            }
        }
        while let Some(([x, y, z], reference)) = queue.pop_front() {
            for axis in 0..3 {
                for step in [-1i64, 1] {
                    let reached = [x, y, z][axis] as i64 + step;
                    if reached < 0 || reached >= dims[axis] as i64 {
                        continue;
                    }
                    let mut neighbour = [x, y, z];
                    neighbour[axis] = reached as usize;
                    self.enter(&mut queue, mask, neighbour, reference);
                }
            }
        }
    }

    fn enter(
        &mut self,
        queue: &mut std::collections::VecDeque<([usize; 3], f32)>,
        mask: &MaskInputs<'_>,
        voxel: [usize; 3],
        reference: f32,
    ) {
        let [x, y, z] = voxel;
        if mask.foreground.get(x, y, z)
            || self.growth.get(x, y, z)
            || !mask.raised.get(x, y, z)
            || mask.admitted.is_some_and(|admitted| !admitted.get(x, y, z))
        {
            return;
        }
        let (background, signal) = mask.field.at(block_of(voxel));
        if signal - background >= mask.field.floor() || background < reference - mask.field.floor()
        {
            return;
        }
        self.growth.set(x, y, z);
        queue.push_back((voxel, reference));
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

    fn flood(
        &mut self,
        foreground: &Bits,
        admitted: Option<&Bits>,
        workers: usize,
        cancel: &std::sync::atomic::AtomicBool,
    ) {
        let blocks = self.blocks;
        let count = blocks[0] * blocks[1] * blocks[2];
        let mut free = vec![[0u64; 8]; count];
        let workers = workers.clamp(1, count.max(1));
        let each = count.div_ceil(workers).max(1);
        crate::schedule::parallel(|scope| {
            for (chunk, slot) in free.chunks_mut(each).zip(0..) {
                let first = slot * each;
                let grid = &*self;
                scope.spawn(move |_| {
                    for (offset, board) in chunk.iter_mut().enumerate() {
                        let index = first + offset;
                        let block = [
                            index % blocks[0],
                            (index / blocks[0]) % blocks[1],
                            index / (blocks[0] * blocks[1]),
                        ];
                        if !grid.barrier(block) || grid.block_outside(block) {
                            continue;
                        }
                        *board = free_board(grid.dims, foreground, admitted, block);
                    }
                });
            }
        });
        let mut entries = vec![[0u64; 8]; count];
        let mut done = vec![[0u64; 8]; count];
        let mut queued = vec![false; count];
        let mut queue = std::collections::VecDeque::new();
        for index in 0..count {
            let block = [
                index % blocks[0],
                (index / blocks[0]) % blocks[1],
                index / (blocks[0] * blocks[1]),
            ];
            if !self.barrier(block) || self.block_outside(block) {
                continue;
            }
            let board = &free[index];
            if board.iter().all(|word| *word == 0) {
                continue;
            }
            let mut seed = [0u64; 8];
            for axis in 0..3 {
                for (offset, high) in [(-1i64, false), (1, true)] {
                    if self.neighbour_outside(block, axis, offset) {
                        or_face(&mut seed, board, axis, high);
                    }
                }
            }
            if seed.iter().all(|word| *word == 0) {
                continue;
            }
            entries[index] = seed;
            queued[index] = true;
            queue.push_back(index);
        }
        while let Some(index) = queue.pop_front() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            queued[index] = false;
            let block = [
                index % blocks[0],
                (index / blocks[0]) % blocks[1],
                index / (blocks[0] * blocks[1]),
            ];
            let reach = closure(&free[index], &entries[index]);
            let mut new = [0u64; 8];
            for z in 0..8 {
                new[z] = reach[z] & !done[index][z];
            }
            if new.iter().all(|word| *word == 0) {
                continue;
            }
            for z in 0..8 {
                done[index][z] |= reach[z];
            }
            mark(&mut self.voxel, self.dims, block, &new);
            for axis in 0..3 {
                for (offset, high) in [(-1i64, false), (1, true)] {
                    let mut neighbour = block;
                    let value = neighbour[axis] as i64 + offset;
                    if value < 0 || value >= blocks[axis] as i64 {
                        continue;
                    }
                    neighbour[axis] = value as usize;
                    if !self.barrier(neighbour) || self.block_outside(neighbour) {
                        continue;
                    }
                    let mut translated = [0u64; 8];
                    translate(&mut translated, &new, axis, high);
                    if translated.iter().all(|word| *word == 0) {
                        continue;
                    }
                    let target =
                        neighbour[0] + blocks[0] * (neighbour[1] + blocks[1] * neighbour[2]);
                    let mut changed = false;
                    for z in 0..8 {
                        if translated[z] & !entries[target][z] != 0 {
                            entries[target][z] |= translated[z];
                            changed = true;
                        }
                    }
                    if changed && !queued[target] {
                        queued[target] = true;
                        queue.push_back(target);
                    }
                }
            }
        }
    }
}

const X_LOW: u64 = 0x0101_0101_0101_0101;
const X_HIGH: u64 = 0x8080_8080_8080_8080;
const Y_LOW: u64 = 0x0000_0000_0000_00ff;
const Y_HIGH: u64 = 0xff00_0000_0000_0000;
const LEFT_MASK: u64 = 0xfefe_fefe_fefe_fefe;
const RIGHT_MASK: u64 = 0x7f7f_7f7f_7f7f_7f7f;

fn free_board(
    dims: [usize; 3],
    foreground: &Bits,
    admitted: Option<&Bits>,
    block: [usize; 3],
) -> [u64; 8] {
    let origin = [block[0] * SIDE, block[1] * SIDE, block[2] * SIDE];
    let limit = [0, 1, 2].map(|axis| (dims[axis] - origin[axis]).min(SIDE));
    let mut board = [0u64; 8];
    for (z, slot) in board.iter_mut().enumerate().take(limit[2]) {
        let mut word = 0u64;
        for y in 0..limit[1] {
            let line = foreground.line(origin[1] + y, origin[2] + z);
            let mut bits = !byte(line, origin[0]) & 0xff;
            if let Some(admitted) = admitted {
                bits &= byte(admitted.line(origin[1] + y, origin[2] + z), origin[0]);
            }
            word |= bits << (8 * y);
        }
        *slot = word;
    }
    board
}

fn byte(line: &[u64], x: usize) -> u64 {
    let word = x / 64;
    let shift = x % 64;
    let mut value = line[word] >> shift;
    if shift + 8 > 64 && word + 1 < line.len() {
        value |= line[word + 1] << (64 - shift);
    }
    value & 0xff
}

fn or_face(seed: &mut [u64; 8], board: &[u64; 8], axis: usize, high: bool) {
    match axis {
        0 => {
            let mask = if high { X_HIGH } else { X_LOW };
            for z in 0..8 {
                seed[z] |= board[z] & mask;
            }
        }
        1 => {
            let mask = if high { Y_HIGH } else { Y_LOW };
            for z in 0..8 {
                seed[z] |= board[z] & mask;
            }
        }
        _ => {
            let z = if high { 7 } else { 0 };
            seed[z] |= board[z];
        }
    }
}

fn translate(target: &mut [u64; 8], board: &[u64; 8], axis: usize, high: bool) {
    match axis {
        0 if high => {
            for z in 0..8 {
                target[z] = (board[z] & X_HIGH) >> 7;
            }
        }
        0 => {
            for z in 0..8 {
                target[z] = (board[z] & X_LOW) << 7;
            }
        }
        1 if high => {
            for z in 0..8 {
                target[z] = (board[z] & Y_HIGH) >> 56;
            }
        }
        1 => {
            for z in 0..8 {
                target[z] = (board[z] & Y_LOW) << 56;
            }
        }
        _ if high => target[0] = board[7],
        _ => target[7] = board[0],
    }
}

fn closure(free: &[u64; 8], entry: &[u64; 8]) -> [u64; 8] {
    let mut reach = [0u64; 8];
    for z in 0..8 {
        reach[z] = entry[z] & free[z];
    }
    loop {
        let mut next = reach;
        for z in 0..8 {
            let word = reach[z];
            next[z] |=
                ((word << 1) & LEFT_MASK) | ((word >> 1) & RIGHT_MASK) | (word << 8) | (word >> 8);
            if z > 0 {
                next[z] |= reach[z - 1];
            }
            if z + 1 < 8 {
                next[z] |= reach[z + 1];
            }
            next[z] &= free[z];
        }
        if next == reach {
            return reach;
        }
        reach = next;
    }
}

fn mark(voxel: &mut Bits, dims: [usize; 3], block: [usize; 3], new: &[u64; 8]) {
    let origin = [block[0] * SIDE, block[1] * SIDE, block[2] * SIDE];
    if origin
        .iter()
        .zip(dims)
        .any(|(value, limit)| *value >= limit)
    {
        return;
    }
    let length = (dims[0] - origin[0]).min(SIDE);
    for (z, word) in new.iter().enumerate() {
        if origin[2] + z >= dims[2] {
            break;
        }
        for y in 0..8 {
            if origin[1] + y >= dims[1] {
                break;
            }
            let bits = (word >> (8 * y)) & 0xff;
            voxel.or_word(origin[0], origin[1] + y, origin[2] + z, bits, length);
        }
    }
}
