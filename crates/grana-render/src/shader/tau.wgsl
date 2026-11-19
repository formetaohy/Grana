@group(3) @binding(0) var tau_output: texture_storage_3d<rgba8unorm, write>;

@group(4) @binding(0) var<uniform> tau_frame: vec4<u32>;
@group(4) @binding(1) var<storage, read> tau_regions: array<vec4<u32>>;

const COLUMN_EPSILON: f32 = 1.0e-6;
const COLUMN_REACH: f32 = 1.0e20;

fn tau_covered(texel: vec2<u32>) -> bool {
    if (tau_frame.y != 0u) {
        return true;
    }
    for (var index = 0u; index < tau_frame.x; index += 1u) {
        let rect = tau_regions[index];
        if (texel.x >= rect.x && texel.x < rect.z && texel.y >= rect.y && texel.y < rect.w) {
            return true;
        }
    }
    return false;
}

struct Column {
    start: vec3<f32>,
    rate: vec3<f32>,
    dims: vec3<f32>,
    band: u32,
    count: u32,
}

struct Block {
    base: vec3<f32>,
    texel: TexelMap,
    packs: u32,
    end: u32,
}

fn column_of(band: u32, uv: vec2<f32>, stride: f32, count: u32) -> Column {
    let g = level_frame(band);
    let origin = (frame.light_inverse * vec4<f32>(uv, 0.0, 1.0)).xyz;
    let advance = (frame.light_inverse * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz;
    let rate = level_rate(band, advance) * g.dims * stride;
    return Column(level_local(band, origin) * g.dims + rate * 0.5, rate, g.dims, band, count);
}

fn reach_layers(column: Column) -> vec2<u32> {
    var low = 0.0;
    var high = f32(column.count);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let rate = column.rate[axis];
        let start = column.start[axis];
        let extent = column.dims[axis];
        if (abs(rate) <= COLUMN_EPSILON) {
            if (start < 0.0 || start > extent) {
                return vec2<u32>(0u);
            }
            continue;
        }
        let first = (0.0 - start) / rate;
        let second = (extent - start) / rate;
        low = max(low, min(first, second));
        high = min(high, max(first, second));
    }
    let start = u32(clamp(ceil(low), 0.0, f32(column.count)));
    let end = u32(clamp(ceil(high), f32(start), f32(column.count)));
    return vec2<u32>(start, end);
}

fn exit_layer(
    column: Column,
    voxel: vec3<f32>,
    low: vec3<f32>,
    high: vec3<f32>,
    layer: u32,
) -> u32 {
    var exit = COLUMN_REACH;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let rate = column.rate[axis];
        if (abs(rate) > COLUMN_EPSILON) {
            let bound = select(low[axis], high[axis], rate > 0.0);
            exit = min(exit, (bound - voxel[axis]) / rate);
        }
    }
    let reach = min(exit, f32(column.count));
    return min(u32(max(ceil(reach), f32(layer) + 1.0)), column.count);
}

fn block_at(column: Column, voxel: vec3<f32>, layer: u32) -> Block {
    let g = level_frame(column.band);
    let cell = brick_of(g.grid, g.dims, voxel);
    let slot = brick_display_slot(level_record(column.band, brick_index(g.grid, cell)));
    let base = brick_origin(cell);
    let far = min(base + vec3<f32>(BRICK_SIDE), g.dims);
    if (slot == 0u) {
        return Block(base, TexelMap(vec3<f32>(0.0), vec3<f32>(1.0)), 0u, exit_layer(column, voxel, base, far, layer));
    }
    let texel = texel_map(slot - 1u, base);
    if (!occupied_brick(slot - 1u)) {
        return Block(base, texel, 0u, exit_layer(column, voxel, base, far, layer));
    }
    let coord = cell_coord(clamp(voxel, base, far) - base);
    let index = cell_index(coord);
    let low = min(base + vec3<f32>(coord) * CELL_SIDE, far);
    let high = min(low + vec3<f32>(CELL_SIDE), far);
    var packs = 0u;
    if (occupied_cell(slot - 1u, index)) {
        packs = occupied_packs(slot - 1u, index) & frame.packs.y;
    }
    let end = select(
        exit_layer(column, voxel, low, high, layer),
        exit_layer(column, voxel, base, far, layer),
        packs == 0u,
    );
    return Block(base, texel, packs, end);
}

fn source_extinction(packs: u32, texel: vec3<f32>) -> f32 {
    var extinction = 0.0;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if ((packs & (1u << pack)) == 0u) {
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

@compute @workgroup_size(4, 4, 1)
fn tau_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(tau_output);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    if (!tau_covered(id.xy)) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + vec2<f32>(0.5)) / vec2<f32>(dims.xy);
    let band = band_count() - 1u;
    let column = column_of(band, uv, 1.0 / f32(dims.z), dims.z);
    let scale = length(column.rate);
    let reach = reach_layers(column);
    var tau = 0.0;
    var layer = 0u;
    while (layer < reach.x) {
        textureStore(
            tau_output,
            vec3<u32>(id.xy, layer),
            vec4<f32>(encoded_tau(tau), 0.0, 0.0, 1.0),
        );
        layer += 1u;
    }
    while (layer < reach.y) {
        let block = block_at(column, column.start + column.rate * f32(layer), layer);
        let stop = min(block.end, reach.y);
        if (block.packs == 0u) {
            while (layer < stop) {
                textureStore(
                    tau_output,
                    vec3<u32>(id.xy, layer),
                    vec4<f32>(encoded_tau(tau), 0.0, 0.0, 1.0),
                );
                layer += 1u;
            }
            continue;
        }
        let far = block.base + vec3<f32>(BRICK_SIDE);
        while (layer < stop) {
            let voxel = column.start + column.rate * f32(layer);
            let texel = texel_at(block.texel, clamp(voxel, block.base, far));
            tau += source_extinction(block.packs, texel) * scale;
            textureStore(
                tau_output,
                vec3<u32>(id.xy, layer),
                vec4<f32>(encoded_tau(tau), 0.0, 0.0, 1.0),
            );
            layer += 1u;
        }
    }
    while (layer < dims.z) {
        textureStore(
            tau_output,
            vec3<u32>(id.xy, layer),
            vec4<f32>(encoded_tau(tau), 0.0, 0.0, 1.0),
        );
        layer += 1u;
    }
}
