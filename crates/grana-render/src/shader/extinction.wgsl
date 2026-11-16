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

fn volume_extinction(band: u32, uvw: vec3<f32>) -> f32 {
    let g = level_frame(band);
    let voxel = uvw * g.dims;
    let cell = brick_of(g.grid, g.dims, voxel);
    let slot = brick_display_slot(level_record(band, brick_index(g.grid, cell)));
    if (slot == 0u) {
        return 0.0;
    }
    let base = brick_origin(cell);
    let local = clamp(voxel, base, base + vec3<f32>(BRICK_SIDE));
    let cells = occupied_packs(slot - 1u, cell_index(cell_coord(local - base))) & frame.packs.y;
    if (cells == 0u) {
        return 0.0;
    }
    let texel = texel_at(texel_map(slot - 1u, base), local);
    var extinction = 0.0;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if ((cells & (1u << pack)) == 0u) {
            continue;
        }
        let values = textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (!channel_active(channel)) {
                continue;
            }
            extinction += channel_extinction(channel, values[component]);
        }
    }
    return extinction;
}
