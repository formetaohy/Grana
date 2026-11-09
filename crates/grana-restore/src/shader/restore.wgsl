struct Dims {
    volume: vec4<u32>,
    kernel: vec4<u32>,
    padded: vec4<u32>,
}

@group(0) @binding(0) var<uniform> dims: Dims;
@group(0) @binding(1) var<storage, read> taps: array<f32>;
@group(0) @binding(2) var<storage, read> bases: array<u32>;
@group(0) @binding(3) var<storage, read> measured: array<f32>;
@group(0) @binding(4) var<storage, read> estimate: array<f32>;
@group(0) @binding(5) var<storage, read_write> updated: array<f32>;
@group(0) @binding(6) var<storage, read_write> blurred: array<f32>;
@group(0) @binding(7) var<storage, read_write> padded_values: array<f32>;

const FLOOR: f32 = 1.0e-6;
const BLOCK: u32 = 4u;

fn cell(x: u32, y: u32, z: u32) -> u32 {
    return x + dims.volume.x * (y + dims.volume.y * z);
}

fn padded_cell(x: u32, y: u32, z: u32) -> u32 {
    return x + dims.padded.w * (y + dims.padded.y * z);
}

fn mirrored(value: i32, limit: u32) -> u32 {
    if (limit <= 1u) {
        return 0u;
    }
    let span = i32(limit);
    var reflected = value;
    if (reflected < 0) {
        reflected = -reflected;
    } else if (reflected >= span) {
        reflected = 2 * (span - 1) - reflected;
    }
    return u32(clamp(reflected, 0, span - 1));
}

fn centre() -> vec3<i32> {
    return vec3<i32>(
        i32(dims.kernel.x / 2u),
        i32(dims.kernel.y / 2u),
        i32(dims.kernel.z / 2u),
    );
}

@compute @workgroup_size(8, 8, 4)
fn pad_estimate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id >= dims.padded.xyz)) {
        return;
    }
    let at = vec3<i32>(id) - centre();
    let source = vec3<u32>(
        mirrored(at.x, dims.volume.x),
        mirrored(at.y, dims.volume.y),
        mirrored(at.z, dims.volume.z),
    );
    padded_values[padded_cell(id.x, id.y, id.z)] = estimate[cell(source.x, source.y, source.z)];
}

@compute @workgroup_size(8, 8, 4)
fn pad_ratio(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id >= dims.padded.xyz)) {
        return;
    }
    let at = vec3<i32>(id) - centre();
    let source = vec3<u32>(
        mirrored(at.x, dims.volume.x),
        mirrored(at.y, dims.volume.y),
        mirrored(at.z, dims.volume.z),
    );
    let index = cell(source.x, source.y, source.z);
    padded_values[padded_cell(id.x, id.y, id.z)] =
        measured[index] / max(blurred[index], FLOOR);
}

@compute @workgroup_size(8, 8, 4)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let x0 = id.x * BLOCK;
    if (x0 >= dims.volume.x || id.y >= dims.volume.y || id.z >= dims.volume.z) {
        return;
    }
    var a0 = 0.0;
    var a1 = 0.0;
    var a2 = 0.0;
    var a3 = 0.0;
    for (var tz = 0u; tz < dims.kernel.z; tz += 1u) {
        let base = bases[id.z + tz];
        for (var ty = 0u; ty < dims.kernel.y; ty += 1u) {
            let row = padded_cell(x0, id.y + ty, id.z + tz);
            let weights = base + dims.kernel.x * (ty + dims.kernel.y * tz);
            var c0 = padded_values[row];
            var c1 = padded_values[row + 1u];
            var c2 = padded_values[row + 2u];
            for (var tx = 0u; tx < dims.kernel.x; tx += 1u) {
                let sample = padded_values[row + tx + 3u];
                let weight = taps[weights + tx];
                a0 += c0 * weight;
                a1 += c1 * weight;
                a2 += c2 * weight;
                a3 += sample * weight;
                c0 = c1;
                c1 = c2;
                c2 = sample;
            }
        }
    }
    let index = cell(x0, id.y, id.z);
    blurred[index] = a0;
    if (x0 + 1u < dims.volume.x) {
        blurred[index + 1u] = a1;
    }
    if (x0 + 2u < dims.volume.x) {
        blurred[index + 2u] = a2;
    }
    if (x0 + 3u < dims.volume.x) {
        blurred[index + 3u] = a3;
    }
}

@compute @workgroup_size(8, 8, 4)
fn correct(@builtin(global_invocation_id) id: vec3<u32>) {
    let x0 = id.x * BLOCK;
    if (x0 >= dims.volume.x || id.y >= dims.volume.y || id.z >= dims.volume.z) {
        return;
    }
    var a0 = 0.0;
    var a1 = 0.0;
    var a2 = 0.0;
    var a3 = 0.0;
    for (var tz = 0u; tz < dims.kernel.z; tz += 1u) {
        let base = bases[id.z + tz];
        let flipped_z = dims.kernel.z - 1u - tz;
        for (var ty = 0u; ty < dims.kernel.y; ty += 1u) {
            let row = padded_cell(x0, id.y + ty, id.z + tz);
            let weights = base + dims.kernel.x * (dims.kernel.y - 1u - ty + dims.kernel.y * flipped_z);
            var c0 = padded_values[row];
            var c1 = padded_values[row + 1u];
            var c2 = padded_values[row + 2u];
            for (var tx = 0u; tx < dims.kernel.x; tx += 1u) {
                let sample = padded_values[row + tx + 3u];
                let weight = taps[weights + dims.kernel.x - 1u - tx];
                a0 += c0 * weight;
                a1 += c1 * weight;
                a2 += c2 * weight;
                a3 += sample * weight;
                c0 = c1;
                c1 = c2;
                c2 = sample;
            }
        }
    }
    let index = cell(x0, id.y, id.z);
    let source = estimate[index];
    updated[index] = source * a0;
    if (x0 + 1u < dims.volume.x) {
        updated[index + 1u] = estimate[index + 1u] * a1;
    }
    if (x0 + 2u < dims.volume.x) {
        updated[index + 2u] = estimate[index + 2u] * a2;
    }
    if (x0 + 3u < dims.volume.x) {
        updated[index + 3u] = estimate[index + 3u] * a3;
    }
}
