@group(5) @binding(0) var<storage, read> occupied: array<f32>;

const OCCUPANCY_WORDS: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;

fn occupancy_word(slot: u32, pack: u32, word: u32) -> f32 {
    return occupied[slot * OCCUPANCY_WORDS * PACK_LIMIT + pack * OCCUPANCY_WORDS + word];
}

fn occupancy_reaches(slot: u32, word: u32) -> bool {
    var reach = 0.0;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (pack_active(pack)) {
            reach = max(reach, occupancy_word(slot, pack, word));
        }
    }
    return reach >= frame.occupancy.x;
}

fn occupied_brick(slot: u32) -> bool {
    return occupancy_reaches(slot, 0u);
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    return occupancy_reaches(slot, 1u + cell);
}

fn occupied_packs(slot: u32, cell: u32) -> u32 {
    var bits = 0u;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (occupancy_word(slot, pack, 1u + cell) >= frame.occupancy.x) {
            bits |= 1u << pack;
        }
    }
    return bits;
}
