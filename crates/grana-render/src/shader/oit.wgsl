@group(4) @binding(0) var<storage, read_write> oit_keys: array<atomic<u32>>;
@group(4) @binding(1) var<storage, read_write> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read_write> oit_counts: array<atomic<u32>>;

const OIT_ALPHA_FLOOR: f32 = 1.0e-3;

fn oit_index(origin: vec2<i32>, size: vec2<i32>, pixel: vec2<i32>) -> i32 {
    let local = pixel - origin;
    if (local.x < 0 || local.y < 0 || local.x >= size.x || local.y >= size.y) {
        return -1;
    }
    return local.y * size.x + local.x;
}

fn oit_insert(index: u32, layers: u32, epoch: u32, depth: f32, premultiplied: vec4<f32>) -> bool {
    let base = index * layers;
    let slot = atomicAdd(&oit_counts[index], 1u);
    if (slot >= layers) {
        return false;
    }
    let key = oit_key(epoch, depth, slot);
    var carry = key;
    var placed = false;
    for (var layer = 0u; layer < layers; layer += 1u) {
        if (carry == 0u) {
            break;
        }
        let stored = atomicMax(&oit_keys[base + layer], carry);
        if (stored == carry) {
            break;
        }
        if (stored < carry) {
            placed = placed || (carry == key);
            carry = stored;
        }
    }
    if (!placed) {
        return false;
    }
    oit_colors[base + slot] = vec2<u32>(
        pack2x16float(premultiplied.rg),
        pack2x16float(premultiplied.ba),
    );
    return true;
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
