@group(4) @binding(0) var<storage, read_write> oit_keys: array<atomic<u32>>;
@group(4) @binding(1) var<storage, read_write> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read_write> oit_counts: array<atomic<u32>>;
@group(4) @binding(3) var<storage, read_write> oit_claims: array<atomic<u32>>;

const OIT_ALPHA_FLOOR: f32 = 1.0e-3;

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

fn oit_offer(index: u32, layers: u32, key: u32) {
    let base = index * layers;
    atomicAdd(&oit_counts[index], 1u);
    var value = key;
    for (var slot = 0u; slot < layers; slot += 1u) {
        if (value == 0u) {
            break;
        }
        let stored = atomicMax(&oit_keys[base + slot], value);
        if (stored == value) {
            break;
        }
        value = min(stored, value);
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

fn oit_claim(index: u32, layers: u32, slot: u32) -> bool {
    let base = index * layers;
    return atomicCompareExchangeWeak(&oit_claims[base + slot], 0u, 1u).exchanged;
}

fn oit_write(index: u32, layers: u32, slot: u32, premultiplied: vec4<f32>) {
    let base = index * layers;
    oit_colors[base + slot] = vec2<u32>(
        pack2x16float(premultiplied.rg),
        pack2x16float(premultiplied.ba),
    );
}

struct Residual {
    @location(0) color: vec4<f32>,
    @location(1) moment: vec2<f32>,
}

fn oit_residual(alpha: f32, color: vec3<f32>, depth: f32) -> Residual {
    var output: Residual;
    output.color = vec4<f32>(0.0);
    output.moment = vec2<f32>(0.0);
    if (alpha <= OIT_ALPHA_FLOOR) {
        return output;
    }
    let tau = -log(max(1.0 - alpha, OIT_ALPHA_FLOOR));
    output.color = vec4<f32>(color * tau, tau);
    output.moment = vec2<f32>(clamp(depth, 0.0, 1.0) * tau, 0.0);
    return output;
}
