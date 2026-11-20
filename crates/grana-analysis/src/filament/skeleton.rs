use super::Grid;
use super::neighbour;

pub(super) fn thin(mask: &mut [u8], distance: &[u16], grid: &Grid) {
    let order = ordered(mask, distance);
    loop {
        let mut changed = false;
        for index in &order {
            let index = *index as usize;
            if mask[index] == 0 {
                continue;
            }
            if simple(mask, index, grid.stride) {
                mask[index] = 0;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

pub(super) fn rejects_sheet(mask: &[u8], grid: &Grid, distance: &[u16]) -> bool {
    let mut foreground = 0usize;
    let mut slabs = 0usize;
    let stride = grid.stride;
    let unit = grid.unit();
    for index in 0..mask.len() {
        if mask[index] == 0 {
            continue;
        }
        foreground += 1;
        let open = neighbour::faces(mask, index, stride).count_ones();
        if open <= 2 && grid.radius(distance[index]) >= 1.5 * unit {
            slabs += 1;
        }
    }
    foreground > 0 && 2 * slabs > foreground
}

fn ordered(mask: &[u8], distance: &[u16]) -> Vec<u32> {
    const BUCKETS: usize = 1 << 16;
    let mut offsets = vec![0u32; BUCKETS + 1];
    for (index, value) in mask.iter().enumerate() {
        if *value != 0 {
            offsets[distance[index] as usize + 1] += 1;
        }
    }
    for value in 0..BUCKETS {
        offsets[value + 1] += offsets[value];
    }
    let mut cursor = offsets.clone();
    let mut order = vec![0u32; offsets[BUCKETS] as usize];
    for (index, value) in mask.iter().enumerate() {
        if *value != 0 {
            let slot = &mut cursor[distance[index] as usize];
            order[*slot as usize] = index as u32;
            *slot += 1;
        }
    }
    order
}

fn simple(mask: &[u8], index: usize, stride: [usize; 3]) -> bool {
    let neighbours = neighbour::neighbourhood(mask, index, stride);
    neighbours.count_ones() >= 2 && neighbour::components(neighbours) == 1
}
