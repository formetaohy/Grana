struct Dims {
    volume: vec4<u32>,
    kernel: vec4<u32>,
    bank: vec4<u32>,
}

@group(0) @binding(0) var<uniform> dims: Dims;
@group(0) @binding(1) var<storage, read> taps: array<f32>;
@group(0) @binding(2) var<storage, read> measured: array<f32>;
@group(0) @binding(3) var<storage, read> estimate: array<f32>;
@group(0) @binding(4) var<storage, read_write> blurred: array<f32>;
@group(0) @binding(5) var<storage, read_write> updated: array<f32>;

const FLOOR: f32 = 1.0e-6;

fn cell(x: u32, y: u32, z: u32) -> u32 {
    return x + dims.volume.x * (y + dims.volume.y * z);
}

fn clamp_axis(value: i32, limit: u32) -> u32 {
    return u32(clamp(value, 0, i32(limit) - 1));
}

fn tap(x: u32, y: u32, z: u32) -> u32 {
    return x + dims.kernel.x * (y + dims.kernel.y * z);
}

fn bank(plane: u32) -> u32 {
    let layer = min((dims.bank.z + plane) / dims.bank.y, dims.bank.x - 1u);
    return layer * dims.kernel.x * dims.kernel.y * dims.kernel.z;
}

@compute @workgroup_size(8, 8, 4)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= dims.volume.x || id.y >= dims.volume.y || id.z >= dims.volume.z) {
        return;
    }
    let extent = dims.kernel.xyz;
    let centre = vec3<i32>(i32(extent.x / 2u), i32(extent.y / 2u), i32(extent.z / 2u));
    let here = vec3<i32>(id);
    var sum = 0.0;
    for (var z = 0u; z < extent.z; z += 1u) {
        let sz = clamp_axis(here.z + i32(z) - centre.z, dims.volume.z);
        let base = bank(sz);
        for (var y = 0u; y < extent.y; y += 1u) {
            let sy = clamp_axis(here.y + i32(y) - centre.y, dims.volume.y);
            for (var x = 0u; x < extent.x; x += 1u) {
                let sx = clamp_axis(here.x + i32(x) - centre.x, dims.volume.x);
                sum += estimate[cell(sx, sy, sz)] * taps[base + tap(x, y, z)];
            }
        }
    }
    blurred[cell(id.x, id.y, id.z)] = sum;
}

@compute @workgroup_size(8, 8, 4)
fn correct(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= dims.volume.x || id.y >= dims.volume.y || id.z >= dims.volume.z) {
        return;
    }
    let extent = dims.kernel.xyz;
    let centre = vec3<i32>(i32(extent.x / 2u), i32(extent.y / 2u), i32(extent.z / 2u));
    let here = vec3<i32>(id);
    var sum = 0.0;
    for (var z = 0u; z < extent.z; z += 1u) {
        let sz = clamp_axis(here.z + i32(z) - centre.z, dims.volume.z);
        let base = bank(sz);
        for (var y = 0u; y < extent.y; y += 1u) {
            let sy = clamp_axis(here.y + i32(y) - centre.y, dims.volume.y);
            for (var x = 0u; x < extent.x; x += 1u) {
                let sx = clamp_axis(here.x + i32(x) - centre.x, dims.volume.x);
                let sample = cell(sx, sy, sz);
                let ratio = measured[sample] / max(blurred[sample], FLOOR);
                sum += ratio * taps[base + tap(extent.x - 1u - x, extent.y - 1u - y, extent.z - 1u - z)];
            }
        }
    }
    let index = cell(id.x, id.y, id.z);
    updated[index] = estimate[index] * sum;
}
