@group(5) @binding(0) var<storage, read> occupied: array<f32>;

const OCCUPANCY_COLUMN: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;
const OCCUPANCY_STRIDE: u32 = OCCUPANCY_COLUMN * (1u + PACK_LIMIT);

fn occupancy_fold(slot: u32, word: u32) -> f32 {
    return occupied[slot * OCCUPANCY_STRIDE + word];
}

fn occupancy_pack(slot: u32, pack: u32, word: u32) -> f32 {
    return occupied[slot * OCCUPANCY_STRIDE + (1u + pack) * OCCUPANCY_COLUMN + word];
}

fn occupied_brick(slot: u32) -> bool {
    return occupancy_fold(slot, 0u) >= frame.occupancy.x;
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    return occupancy_fold(slot, 1u + cell) >= frame.occupancy.x;
}

fn occupied_packs(slot: u32, cell: u32) -> u32 {
    var bits = 0u;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (occupancy_pack(slot, pack, 1u + cell) >= frame.occupancy.x) {
            bits |= 1u << pack;
        }
    }
    return bits;
}
