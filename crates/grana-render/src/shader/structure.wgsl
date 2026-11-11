struct BakeEntry {
    band: u32,
    slot: u32,
    brick: u32,
    reserved: u32,
}

@group(5) @binding(0) var<storage, read> bake_entries: array<BakeEntry>;
@group(5) @binding(1) var structure_output: texture_storage_3d<rgba16float, write>;

const BAKE_TILE: u32 = 4u;
const BAKE_TILES: u32 = 9u;

fn sign_not_zero(axis: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(select(-1.0, 1.0, axis.x >= 0.0), select(-1.0, 1.0, axis.y >= 0.0));
}

fn octahedral_encode(normal: vec3<f32>) -> vec2<f32> {
    let unit = normal / max(abs(normal.x) + abs(normal.y) + abs(normal.z), 1.0e-8);
    if (unit.z >= 0.0) {
        return unit.xy;
    }
    return (vec2<f32>(1.0) - abs(unit.yx)) * sign_not_zero(unit.xy);
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
    return vec4<f32>(packed.x, packed.y, 1.0, 0.0);
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
    let local = vec3<i32>(
        i32(group.x * BAKE_TILE + lane.x),
        i32(group.y * BAKE_TILE + lane.y),
        i32((group.z % BAKE_TILES) * BAKE_TILE + lane.z),
    );
    let side = i32(PADDED_SIDE);
    if (any(local >= vec3<i32>(side))) {
        return;
    }
    let origin = vec3<i32>(slot_origin(frame.atlas.xyz, entry.slot));
    var structure = vec4<f32>(0.0);
    if (structure_density(origin + local) > 0.0) {
        let inner = clamp(local, vec3<i32>(1), vec3<i32>(side - 2));
        structure = structure_normal(entry.band, origin, inner);
    }
    textureStore(structure_output, origin + local, structure);
}
