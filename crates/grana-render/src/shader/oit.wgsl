@group(4) @binding(0) var<storage, read_write> oit_keys: array<atomic<u32>>;
@group(4) @binding(1) var<storage, read_write> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read_write> oit_counts: array<atomic<u32>>;
@group(4) @binding(3) var<storage, read_write> oit_dropped: atomic<u32>;
@group(6) @binding(0) var oit_coverage: texture_2d<u32>;

const OIT_WEIGHT_FLOOR: f32 = 1.0e-2;
const OIT_WEIGHT_CEIL: f32 = 3.0e3;

fn oit_key(depth: f32) -> u32 {
    return ~bitcast<u32>(clamp(depth, 0.0, 1.0));
}

fn oit_index(origin: vec2<i32>, size: vec2<i32>, pixel: vec2<i32>) -> i32 {
    let local = pixel - origin;
    if (local.x < 0 || local.y < 0 || local.x >= size.x || local.y >= size.y) {
        return -1;
    }
    return local.y * size.x + local.x;
}

fn oit_covered(pixel: vec2<i32>) -> f32 {
    return f32(textureLoad(oit_coverage, pixel, 0).r);
}

fn oit_offer(index: u32, layers: u32, key: u32) {
    let base = index * layers;
    atomicAdd(&oit_counts[index], 1u);
    var value = key;
    for (var slot = 0u; slot < layers; slot += 1u) {
        let stored = atomicMax(&oit_keys[base + slot], value);
        value = min(stored, value);
    }
    if (value != 0u) {
        atomicAdd(&oit_dropped, 1u);
    }
}

fn oit_slot(index: u32, layers: u32, key: u32) -> i32 {
    let base = index * layers;
    let stored = min(atomicLoad(&oit_counts[index]), layers);
    for (var slot = 0u; slot < stored; slot += 1u) {
        if (atomicLoad(&oit_keys[base + slot]) == key) {
            return i32(slot);
        }
    }
    return -1;
}

fn oit_beyond(index: u32, layers: u32, key: u32) -> bool {
    let base = index * layers;
    let stored = min(atomicLoad(&oit_counts[index]), layers);
    if (stored == 0u) {
        return true;
    }
    var farthest = 0xffffffffu;
    for (var slot = 0u; slot < stored; slot += 1u) {
        let loaded = atomicLoad(&oit_keys[base + slot]);
        if (loaded != 0u) {
            farthest = min(farthest, loaded);
        }
    }
    return key < farthest;
}

fn oit_write(index: u32, layers: u32, slot: u32, premultiplied: vec4<f32>) {
    let base = index * layers;
    oit_colors[base + slot] = vec2<u32>(
        pack2x16float(premultiplied.rg),
        pack2x16float(premultiplied.ba),
    );
}

fn oit_weight(depth: f32, alpha: f32) -> f32 {
    let reach = clamp(depth, 0.0, 1.0);
    let falloff = 0.03 / (1.0e-5 + reach * reach * reach * reach);
    return alpha * clamp(falloff, OIT_WEIGHT_FLOOR, OIT_WEIGHT_CEIL);
}
