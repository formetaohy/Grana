@group(0) @binding(0) var<storage, read> partials: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> total: vec2<f32>;
@group(0) @binding(2) var<uniform> counts: vec4<u32>;

const GROUP: u32 = 256u;

var<workgroup> scratch: array<vec2<f32>, GROUP>;

@compute @workgroup_size(GROUP)
fn reduce(@builtin(local_invocation_index) local: u32) {
    var sum = vec2<f32>(0.0);
    var index = local;
    loop {
        if (index >= counts.x) {
            break;
        }
        sum += partials[index];
        index += GROUP;
    }
    scratch[local] = sum;
    workgroupBarrier();
    for (var step = GROUP / 2u; step > 0u; step = step / 2u) {
        if (local < step) {
            scratch[local] += scratch[local + step];
        }
        workgroupBarrier();
    }
    if (local == 0u) {
        total = scratch[0];
    }
}
