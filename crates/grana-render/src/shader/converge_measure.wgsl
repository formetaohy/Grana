@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var reference: texture_2d<f32>;
@group(0) @binding(2) var<uniform> frame: vec4<u32>;
@group(0) @binding(3) var<storage, read_write> partials: array<vec2<f32>>;

const COVERAGE: f32 = 0.01;
const MINIMUM_ALPHA: f32 = 1.0e-4;
const DEVIATION_LIMIT: f32 = 8.0;
const RADIANCE_FLOOR: f32 = 0.01;
const GROUP: u32 = 256u;

var<workgroup> scratch: array<vec2<f32>, GROUP>;

fn deviation(pixel: vec2<i32>) -> vec2<f32> {
    let sample = textureLoad(source, pixel, 0);
    let sum = textureLoad(reference, pixel, 0);
    if (sum.a <= COVERAGE) {
        return vec2<f32>(0.0, 0.0);
    }
    let straight = sample.rgb / max(sample.a, MINIMUM_ALPHA);
    let mean = sum.rgb / sum.a;
    let delta = max(
        abs(straight.r - mean.r),
        max(abs(straight.g - mean.g), abs(straight.b - mean.b)),
    );
    let scale = max(max(mean.r, max(mean.g, mean.b)), RADIANCE_FLOOR);
    return vec2<f32>(min(delta / scale, DEVIATION_LIMIT), 1.0);
}

@compute @workgroup_size(GROUP)
fn measure(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let size = frame.zw;
    let pixels = size.x * size.y;
    let stride = groups.x * GROUP;
    var total = vec2<f32>(0.0);
    var index = group.x * GROUP + local;
    loop {
        if (index >= pixels) {
            break;
        }
        let pixel = vec2<u32>(index % size.x, index / size.x);
        total += deviation(vec2<i32>(pixel) + vec2<i32>(frame.xy));
        index += stride;
    }
    scratch[local] = total;
    workgroupBarrier();
    for (var step = GROUP / 2u; step > 0u; step = step / 2u) {
        if (local < step) {
            scratch[local] += scratch[local + step];
        }
        workgroupBarrier();
    }
    if (local == 0u) {
        partials[group.x] = scratch[0];
    }
}
