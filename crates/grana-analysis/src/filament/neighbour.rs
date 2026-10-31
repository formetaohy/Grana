pub(super) const COUNT: usize = 26;

pub(super) const OFFSET: [[isize; 3]; COUNT] = offsets();

pub(super) const ADJACENT: [u32; COUNT] = adjacent();

pub(super) const HALF: [usize; 13] = half();

const fn offsets() -> [[isize; 3]; COUNT] {
    let mut table = [[0isize; 3]; COUNT];
    let mut index = 0;
    let mut z = -1isize;
    while z <= 1 {
        let mut y = -1isize;
        while y <= 1 {
            let mut x = -1isize;
            while x <= 1 {
                if x != 0 || y != 0 || z != 0 {
                    table[index] = [x, y, z];
                    index += 1;
                }
                x += 1;
            }
            y += 1;
        }
        z += 1;
    }
    table
}

const fn adjacent() -> [u32; COUNT] {
    let mut table = [0u32; COUNT];
    let mut left = 0;
    while left < COUNT {
        let mut right = 0;
        while right < COUNT {
            if left != right {
                let mut reach = 0isize;
                let mut axis = 0;
                while axis < 3 {
                    let delta = OFFSET[left][axis] - OFFSET[right][axis];
                    let magnitude = if delta < 0 { -delta } else { delta };
                    if magnitude > reach {
                        reach = magnitude;
                    }
                    axis += 1;
                }
                if reach <= 1 {
                    table[left] |= 1u32 << right;
                }
            }
            right += 1;
        }
        left += 1;
    }
    table
}

const fn half() -> [usize; 13] {
    let mut table = [0usize; 13];
    let mut index = 0;
    let mut found = 0;
    while index < COUNT {
        let offset = OFFSET[index];
        if offset[2] > 0 || (offset[2] == 0 && (offset[1] > 0 || (offset[1] == 0 && offset[0] > 0)))
        {
            table[found] = index;
            found += 1;
        }
        index += 1;
    }
    table
}

pub(super) fn components(mut neighbours: u32) -> u32 {
    let mut count = 0;
    while neighbours != 0 {
        count += 1;
        let mut stack = 1u32 << neighbours.trailing_zeros();
        neighbours &= !stack;
        while stack != 0 {
            let position = stack.trailing_zeros();
            stack &= stack - 1;
            let joined = ADJACENT[position as usize] & neighbours;
            neighbours &= !joined;
            stack |= joined;
        }
    }
    count
}

pub(super) fn neighbourhood(mask: &[u8], index: usize, stride: [usize; 3]) -> u32 {
    let mut neighbours = 0u32;
    for (position, offset) in OFFSET.iter().enumerate() {
        let neighbour = offset[0] * stride[0] as isize
            + offset[1] * stride[1] as isize
            + offset[2] * stride[2] as isize;
        let target = (index as isize + neighbour) as usize;
        if mask[target] != 0 {
            neighbours |= 1u32 << position;
        }
    }
    neighbours
}
