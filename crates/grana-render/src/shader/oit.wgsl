@group(4) @binding(0) var<storage, read_write> oit_keys: array<u32>;
@group(4) @binding(1) var<storage, read_write> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read_write> oit_counts: array<atomic<u32>>;

const OIT_WEIGHT_FLOOR: f32 = 1.0e-2;
const OIT_WEIGHT_CEIL: f32 = 3.0e3;

fn oit_key(depth: f32) -> u32 {
    return ~bitcast<u32>(clamp(depth, 0.0, 1.0));
}

fn oit_inside(origin: vec2<i32>, size: vec2<i32>, pixel: vec2<i32>) -> bool {
    let local = pixel - origin;
    return local.x >= 0 && local.y >= 0 && local.x < size.x && local.y < size.y;
}

fn oit_pixel(origin: vec2<i32>, size: vec2<i32>, pixel: vec2<i32>) -> u32 {
    let local = pixel - origin;
    return u32(local.y) * u32(size.x) + u32(local.x);
}

fn oit_push(
    origin: vec2<i32>,
    size: vec2<i32>,
    layers: u32,
    pixel: vec2<i32>,
    depth: f32,
    color: vec4<f32>,
) {
    if (color.a <= 0.0 || !oit_inside(origin, size, pixel)) {
        return;
    }
    let index = oit_pixel(origin, size, pixel);
    let slot = atomicAdd(&oit_counts[index], 1u);
    if (slot >= layers) {
        return;
    }
    let base = index * layers + slot;
    oit_keys[base] = oit_key(depth);
    oit_colors[base] = vec2<u32>(pack2x16float(color.rg), pack2x16float(color.ba));
}

fn oit_tail(
    origin: vec2<i32>,
    size: vec2<i32>,
    layers: u32,
    pixel: vec2<i32>,
    depth: f32,
) -> bool {
    if (!oit_inside(origin, size, pixel)) {
        return false;
    }
    let index = oit_pixel(origin, size, pixel);
    let stored = min(atomicLoad(&oit_counts[index]), layers);
    let key = oit_key(depth);
    let base = index * layers;
    for (var slot = 0u; slot < stored; slot += 1u) {
        if (oit_keys[base + slot] == key) {
            return false;
        }
    }
    return true;
}

fn oit_weight(depth: f32, alpha: f32) -> f32 {
    let reach = clamp(depth, 0.0, 1.0);
    let falloff = 0.03 / (1.0e-5 + reach * reach * reach * reach);
    return alpha * clamp(falloff, OIT_WEIGHT_FLOOR, OIT_WEIGHT_CEIL);
}
