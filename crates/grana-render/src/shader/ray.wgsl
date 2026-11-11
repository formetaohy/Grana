@group(3) @binding(0) var object_depth: texture_depth_2d;
@group(4) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(4) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(4) @binding(3) var oit_accumulate: texture_2d<f32>;
@group(4) @binding(4) var oit_reveal: texture_2d<f32>;
@group(4) @binding(5) var oit_tail_depth: texture_2d<f32>;

const EMPTY_DISTANCE: f32 = 1.0e19;
const MAXIMUM_ENTRIES: u32 = 9u;

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

struct Layers {
    depth: array<f32, MAXIMUM_ENTRIES>,
    color: array<vec2<u32>, MAXIMUM_ENTRIES>,
    count: u32,
}

struct Tail {
    color: vec3<f32>,
    alpha: f32,
    depth: f32,
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

fn gather_layers(clip_position: vec4<f32>) -> Layers {
    var layers: Layers;
    layers.count = 0u;
    if (frame.oit.x == 0u) {
        return layers;
    }
    let scene_pixel = vec2<i32>(floor(clip_position.xy));
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let local = scene_pixel - origin;
    if (local.x < 0 || local.y < 0 || local.x >= size.x || local.y >= size.y) {
        return layers;
    }
    let index = u32(local.y) * u32(size.x) + u32(local.x);
    let base = index * frame.oit.y;
    let stored = min(oit_counts[index], frame.oit.y);
    for (var slot = 0u; slot < stored; slot += 1u) {
        let key = oit_keys[base + slot];
        if (key == 0u) {
            break;
        }
        layers.depth[layers.count] = bitcast<f32>(~key);
        layers.color[layers.count] = oit_colors[base + slot];
        layers.count += 1u;
    }
    return layers;
}

fn tail_layer(local: vec2<i32>) -> Tail {
    let scale = max(frame.oit.w, 1u);
    let coordinate = (vec2<f32>(local) + 0.5) / f32(scale) - 0.5;
    let base = vec2<i32>(floor(coordinate));
    let fraction = coordinate - floor(coordinate);
    let extent = vec2<i32>(textureDimensions(oit_accumulate)) - vec2<i32>(1);
    let low = clamp(base, vec2<i32>(0), extent);
    let high = clamp(base + vec2<i32>(1), vec2<i32>(0), extent);
    let across = vec2<i32>(high.x, low.y);
    let down = vec2<i32>(low.x, high.y);
    let accumulation = mix(
        mix(
            textureLoad(oit_accumulate, low, 0),
            textureLoad(oit_accumulate, across, 0),
            fraction.x,
        ),
        mix(
            textureLoad(oit_accumulate, down, 0),
            textureLoad(oit_accumulate, high, 0),
            fraction.x,
        ),
        fraction.y,
    );
    if (accumulation.a <= 0.0) {
        return Tail(vec3<f32>(0.0), 0.0, EMPTY_DISTANCE);
    }
    let reveal = mix(
        mix(
            textureLoad(oit_reveal, low, 0).r,
            textureLoad(oit_reveal, across, 0).r,
            fraction.x,
        ),
        mix(
            textureLoad(oit_reveal, down, 0).r,
            textureLoad(oit_reveal, high, 0).r,
            fraction.x,
        ),
        fraction.y,
    );
    let nearest = min(
        min(
            textureLoad(oit_tail_depth, low, 0).x,
            textureLoad(oit_tail_depth, across, 0).x,
        ),
        min(
            textureLoad(oit_tail_depth, down, 0).x,
            textureLoad(oit_tail_depth, high, 0).x,
        ),
    );
    return Tail(
        accumulation.rgb / accumulation.a,
        1.0 - reveal,
        select(EMPTY_DISTANCE, nearest, nearest < 1.0),
    );
}

fn append_tail(layers: Layers, clip_position: vec4<f32>) -> Layers {
    var result = layers;
    if (frame.oit.x == 0u) {
        return result;
    }
    let scene_pixel = vec2<i32>(floor(clip_position.xy));
    let local = scene_pixel - vec2<i32>(frame.viewport.xy);
    if (local.x < 0
        || local.y < 0
        || local.x >= i32(frame.viewport.z)
        || local.y >= i32(frame.viewport.w)) {
        return result;
    }
    let tail = tail_layer(local);
    if (tail.alpha <= 0.0) {
        return result;
    }
    var depth = tail.depth;
    if (result.count > 0u) {
        depth = max(depth, result.depth[result.count - 1u]);
    }
    let premultiplied = vec4<f32>(tail.color * tail.alpha, tail.alpha);
    let at = result.count;
    result.depth[at] = depth;
    result.color[at] = vec2<u32>(
        pack2x16float(premultiplied.rg),
        pack2x16float(premultiplied.ba),
    );
    result.count = at + 1u;
    return result;
}
