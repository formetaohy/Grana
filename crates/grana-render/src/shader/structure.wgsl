struct BakeEntry {
    band: u32,
    slot: u32,
    brick: u32,
    reserved: u32,
}

@group(5) @binding(0) var<storage, read> bake_entries: array<BakeEntry>;
@group(5) @binding(1) var structure_output: texture_storage_3d<rgba8unorm, write>;

const BAKE_TILE: u32 = 4u;
const BAKE_TILES: u32 = 5u;

const STRUCTURE_STRIDE: i32 = 2;

const STRUCTURE_PAD: i32 = 1;

fn structure_density(texel: vec3<i32>) -> f32 {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var total = 0.0;
    for (var pack = 0u; pack < packs; pack += 1u) {
        let values = textureLoad(atlases[pack], texel, 0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            total += max(values[component], 0.0);
        }
    }
    return total;
}

fn structure_normal(band: u32, origin: vec3<i32>, inner: vec3<i32>) -> vec4<f32> {
    let dx = structure_density(origin + inner + vec3<i32>(1, 0, 0))
        - structure_density(origin + inner - vec3<i32>(1, 0, 0));
    let dy = structure_density(origin + inner + vec3<i32>(0, 1, 0))
        - structure_density(origin + inner - vec3<i32>(0, 1, 0));
    let dz = structure_density(origin + inner + vec3<i32>(0, 0, 1))
        - structure_density(origin + inner - vec3<i32>(0, 0, 1));
    let gradient = level_gradient(band, vec3<f32>(dx, dy, dz) / frame.dims[band].xyz);
    if (dot(gradient, gradient) <= 0.0) {
        return vec4<f32>(0.0);
    }
    let packed = octahedral_encode(normalize(-gradient));
    return vec4<f32>(packed * 0.5 + vec2<f32>(0.5), 1.0, 0.0);
}

fn structure_origin(grid: vec3<u32>, slot: u32, side: i32) -> vec3<i32> {
    let layer = slot / (grid.x * grid.y);
    let remainder = slot % (grid.x * grid.y);
    let row = remainder / grid.x;
    let column = remainder % grid.x;
    return vec3<i32>(i32(column), i32(row), i32(layer)) * side;
}

@compute @workgroup_size(4, 4, 4)
fn bake_main(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_id) lane: vec3<u32>,
) {
    let index = group.z / BAKE_TILES;
    if (index >= arrayLength(&bake_entries)) {
        return;
    }
    let entry = bake_entries[index];
    let cell = vec3<i32>(
        i32(group.x * BAKE_TILE + lane.x),
        i32(group.y * BAKE_TILE + lane.y),
        i32((group.z % BAKE_TILES) * BAKE_TILE + lane.z),
    );
    let dims = vec3<i32>(textureDimensions(structure_output));
    let side = dims.x / i32(frame.atlas.x);
    if (any(cell >= vec3<i32>(side))) {
        return;
    }
    let origin = vec3<i32>(slot_origin(frame.atlas.xyz, entry.slot));
    let inner = clamp(cell, vec3<i32>(STRUCTURE_PAD), vec3<i32>(side - 1 - STRUCTURE_PAD));
    let centre = (inner - vec3<i32>(STRUCTURE_PAD)) * STRUCTURE_STRIDE;
    let limit = vec3<i32>(i32(PADDED_SIDE) - 2);
    var total = vec3<f32>(0.0);
    var coverage = 0.0;
    for (var z = 0; z < STRUCTURE_STRIDE; z += 1) {
        for (var y = 0; y < STRUCTURE_STRIDE; y += 1) {
            for (var x = 0; x < STRUCTURE_STRIDE; x += 1) {
                let texel = centre + vec3<i32>(x, y, z);
                if (structure_density(origin + texel) <= 0.0) {
                    continue;
                }
                let held = structure_normal(entry.band, origin, clamp(texel, vec3<i32>(1), limit));
                if (held.z <= 0.0) {
                    continue;
                }
                total += octahedral_decode(held.xy * 2.0 - vec2<f32>(1.0));
                coverage += 1.0;
            }
        }
    }
    var structure = vec4<f32>(0.0);
    if (coverage > 0.0 && dot(total, total) > 0.0) {
        let packed = octahedral_encode(normalize(total));
        structure = vec4<f32>(
            packed * 0.5 + vec2<f32>(0.5),
            coverage / f32(STRUCTURE_STRIDE * STRUCTURE_STRIDE * STRUCTURE_STRIDE),
            0.0,
        );
    }
    textureStore(
        structure_output,
        structure_origin(frame.atlas.xyz, entry.slot, side) + cell,
        structure,
    );
}
