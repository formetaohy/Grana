@group(5) @binding(0) var<storage, read> occupied: array<u32>;

const OCCUPANCY_COLUMN: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;
const OCCUPANCY_UNIT: f32 = 1.0 / 65535.0;
const OCCUPANCY_MASK: u32 = (1u << PACK_LIMIT) - 1u;

fn occupancy_word(slot: u32, word: u32) -> u32 {
    return occupied[slot * OCCUPANCY_COLUMN + word];
}

fn occupancy_value(word: u32) -> f32 {
    return f32(word & 0xffffu) * OCCUPANCY_UNIT;
}

fn occupancy_packs(word: u32) -> u32 {
    return (word >> 16u) & OCCUPANCY_MASK;
}

fn occupied_brick(slot: u32) -> bool {
    return occupancy_value(occupancy_word(slot, 0u)) >= frame.occupancy.x;
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    return occupancy_value(occupancy_word(slot, 1u + cell)) >= frame.occupancy.x;
}

fn occupied_packs(slot: u32, cell: u32) -> u32 {
    return occupancy_packs(occupancy_word(slot, 1u + cell));
}
