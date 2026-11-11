@group(4) @binding(0) var shadow_tau: texture_3d<f32>;
@group(4) @binding(1) var shadow_sampler: sampler;
@group(4) @binding(3) var shadow_objects: texture_depth_2d;
@group(4) @binding(4) var shadow_compare: sampler_comparison;

struct BakeEntry {
    band: u32,
    slot: u32,
    brick: u32,
    reserved: u32,
}

@group(5) @binding(0) var<storage, read> bake_entries: array<BakeEntry>;
@group(5) @binding(1) var baked_output: texture_storage_3d<rgba16float, write>;
@group(5) @binding(2) var<storage, read_write> occupied: array<atomic<u32>>;

const OCCUPANCY_WORDS: u32 = 65u;
const BAKE_TILE: u32 = 4u;
const BAKE_TILES: u32 = 9u;
const BAKE_MINIMUM_TAU: f32 = 1.0e-4;
const BAKE_ISO_EXTINCTION: f32 = 8.0;
const BAKE_ISO_SOFTNESS: f32 = 0.04;

fn iso_density(channel: u32, value: f32) -> f32 {
    let inverse = channels[channel].inverse;
    if (inverse <= 0.0) {
        return 0.0;
    }
    let softness = BAKE_ISO_SOFTNESS / inverse;
    let threshold = channel_threshold(channel);
    return BAKE_ISO_EXTINCTION * smoothstep(threshold - softness, threshold + softness, value);
}

fn classified(texel: vec3<i32>) -> vec4<f32> {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var tau = 0.0;
    var emission = vec3<f32>(0.0);
    for (var pack = 0u; pack < packs; pack += 1u) {
        let values = textureLoad(atlases[pack], texel, 0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0 || channel_maximum(channel)) {
                continue;
            }
            let value = values[component];
            if (channel_isosurface(channel)) {
                let density = iso_density(channel, value) * opacity;
                tau += density;
                emission += density * channel_tau(channel, value).rgb;
                continue;
            }
            if (value < channel_threshold(channel)) {
                continue;
            }
            let extinction = channel_tau(channel, value);
            let weight = extinction.a * opacity;
            tau += weight;
            emission += weight * extinction.rgb;
        }
    }
    return vec4<f32>(emission / max(tau, 1.0e-8), tau);
}

fn bake_light(color: vec3<f32>, normal: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let light = frame.light.xyz;
    let visibility = sun_transmittance(
        shadow_coordinate(frame.light_view, world),
        shadow_bias(frame.light_reach, normal, light),
    );
    let cosine = max(dot(normal, light), 0.0);
    return color * (frame.shading.x * scene_environment(normal) + frame.shading.y * cosine * visibility);
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
    let grid = vec3<i32>(frame.grids[entry.band].xyz);
    let brick = vec3<i32>(
        i32(entry.brick) % grid.x,
        (i32(entry.brick) / grid.x) % grid.y,
        i32(entry.brick) / (grid.x * grid.y),
    );
    let origin = vec3<i32>(slot_origin(frame.atlas.xyz, entry.slot));
    let center = classified(origin + local);
    let dims = frame.dims[entry.band].xyz;
    let voxel = vec3<f32>(brick) * BRICK_SIDE + vec3<f32>(local) - vec3<f32>(0.5);
    var color = center.rgb;
    if (center.a > BAKE_MINIMUM_TAU) {
        let inner = clamp(local, vec3<i32>(1), vec3<i32>(side - 2));
        let dx = classified(origin + inner + vec3<i32>(1, 0, 0)).a
            - classified(origin + inner - vec3<i32>(1, 0, 0)).a;
        let dy = classified(origin + inner + vec3<i32>(0, 1, 0)).a
            - classified(origin + inner - vec3<i32>(0, 1, 0)).a;
        let dz = classified(origin + inner + vec3<i32>(0, 0, 1)).a
            - classified(origin + inner - vec3<i32>(0, 0, 1)).a;
        let gradient = vec3<f32>(dx, dy, dz) / dims;
        if (dot(gradient, gradient) > 0.0) {
            var normal = normalize(-level_gradient(entry.band, gradient));
            if (dot(normal, frame.light.xyz) < 0.0) {
                normal = -normal;
            }
            let uvw = clamp(voxel / dims, vec3<f32>(0.0), vec3<f32>(1.0));
            color = bake_light(color, normal, level_world(entry.band, uvw));
        }
    }
    textureStore(baked_output, origin + local, vec4<f32>(color, center.a));
    let cell = clamp(
        (local - vec3<i32>(1)) / i32(CELL_SIDE),
        vec3<i32>(0),
        vec3<i32>(i32(CELL_AXIS) - 1),
    );
    let cell_index = u32(cell.x + 4 * (cell.y + 4 * cell.z));
    let base = entry.slot * OCCUPANCY_WORDS;
    let bits = bitcast<u32>(center.a);
    atomicMax(&occupied[base], bits);
    atomicMax(&occupied[base + 1u + cell_index], bits);
}
