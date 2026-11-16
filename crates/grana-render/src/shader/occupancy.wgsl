@group(5) @binding(0) var<storage, read> occupied: array<u32>;

const OCCUPANCY_WORDS: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;

const OCCUPANCY_PACKS: u32 = 4u;

fn occupied_brick(slot: u32) -> bool {
    for (var pack = 0u; pack < OCCUPANCY_PACKS; pack += 1u) {
        let word = (slot * OCCUPANCY_PACKS + pack) * OCCUPANCY_WORDS;
        if (occupancy_reaches(occupied[word])) {
            return true;
        }
    }
    return false;
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    for (var pack = 0u; pack < OCCUPANCY_PACKS; pack += 1u) {
        let word = (slot * OCCUPANCY_PACKS + pack) * OCCUPANCY_WORDS + 1u + cell;
        if (occupancy_reaches(occupied[word])) {
            return true;
        }
    }
    return false;
}
