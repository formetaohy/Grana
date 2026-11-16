@group(5) @binding(0) var<storage, read> occupied: array<vec4<f32>>;

const OCCUPANCY_WORDS: u32 = 1u + CELL_GRID * CELL_GRID * CELL_GRID;

fn occupancy_reaches(word: vec4<f32>) -> bool {
    return max(max(word.x, word.y), max(word.z, word.w)) >= frame.occupancy.x;
}

fn occupied_brick(slot: u32) -> bool {
    return occupancy_reaches(occupied[slot * OCCUPANCY_WORDS]);
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    return occupancy_reaches(occupied[slot * OCCUPANCY_WORDS + 1u + cell]);
}
