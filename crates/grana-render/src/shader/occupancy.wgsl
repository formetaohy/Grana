@group(5) @binding(0) var<storage, read> occupied: array<vec4<f32>>;

const OCCUPANCY_WORDS: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;

fn occupancy_reaches(word: vec4<f32>) -> bool {
    var reach = 0.0;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (pack_active(pack)) {
            reach = max(reach, word[pack]);
        }
    }
    return reach >= frame.occupancy.x;
}

fn occupied_brick(slot: u32) -> bool {
    return occupancy_reaches(occupied[slot * OCCUPANCY_WORDS]);
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    return occupancy_reaches(occupied[slot * OCCUPANCY_WORDS + 1u + cell]);
}

fn occupied_packs(slot: u32, cell: u32) -> u32 {
    let word = occupied[slot * OCCUPANCY_WORDS + 1u + cell];
    var bits = 0u;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (word[pack] >= frame.occupancy.x) {
            bits |= 1u << pack;
        }
    }
    return bits;
}
