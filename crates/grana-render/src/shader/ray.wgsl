@group(3) @binding(0) var object_depth: texture_depth_2d;

const EMPTY_DISTANCE: f32 = 1.0e19;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

struct VolumeOutput {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

struct Resolution {
    origin: vec3<f32>,
    direction: vec3<f32>,
    start: f32,
    exit: f32,
    jitter: f32,
    dither: f32,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn object_distance(origin: vec3<f32>, depth: f32, ndc: vec2<f32>) -> f32 {
    if (depth >= 1.0) {
        return EMPTY_DISTANCE;
    }
    return length(unproject(vec3<f32>(ndc, depth)) - origin);
}

fn region_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    if (!region_active()) {
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    let offset = origin - frame.region_center.xyz;
    var entry = -1.0e20;
    var exit = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let unit = frame.region_axis[axis].xyz;
        let half = frame.region_axis[axis].w;
        let start = dot(offset, unit);
        let step = dot(direction, unit);
        if (abs(step) < 1.0e-9) {
            if (abs(start) > half) {
                return vec2<f32>(1.0, -1.0);
            }
        } else {
            let inverse = 1.0 / step;
            let first = (-half - start) * inverse;
            let second = (half - start) * inverse;
            entry = max(entry, min(first, second));
            exit = min(exit, max(first, second));
        }
    }
    return vec2<f32>(entry, exit);
}

fn clip_depth(world: vec3<f32>) -> f32 {
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    return clamp(clip.z / clip.w, 0.0, 1.0);
}

fn resolve(clip_position: vec4<f32>, ndc: vec2<f32>) -> Resolution {
    let scene_pixel = vec2<i32>(floor(clip_position.xy));
    let ray = camera_ray(ndc);
    let origin = ray.origin;
    let direction = ray.direction;
    let local_eye = level_local(0u, origin);
    let local_direction = level_rate(0u, direction);
    let inverse_direction = vec3<f32>(1.0) / local_direction;
    let near_t = (vec3<f32>(0.0) - local_eye) * inverse_direction;
    let far_t = (vec3<f32>(1.0) - local_eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(origin, textureLoad(object_depth, scene_pixel, 0), ndc);
    let region = region_interval(origin, direction);
    let plane = clip_interval(origin, direction);
    let clip = vec2<f32>(max(region.x, plane.x), min(region.y, plane.y));
    let exit = min(
        min(min(highest_t.x, highest_t.y), highest_t.z),
        min(min(object, clip.y), 1.0e19),
    );
    let start = max(max(entry, 0.0), clip.x);
    let jitter = sample_noise(clip_position.xy, frame.jitter.z) * local_step(start);
    let dither = sample_noise(clip_position.xy, frame.jitter.z + 8.0);
    return Resolution(origin, direction, start, exit, jitter, dither);
}
