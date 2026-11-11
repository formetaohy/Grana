@group(4) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(4) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(4) @binding(3) var oit_accumulate: texture_2d<f32>;
@group(4) @binding(4) var oit_reveal: texture_2d<f32>;
@group(4) @binding(5) var oit_tail_depth: texture_2d<f32>;

const MAXIMUM_ENTRIES: u32 = 9u;

const MINIMUM_TRANSMITTANCE: f32 = 0.003;

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
        return Tail(vec3<f32>(0.0), 0.0, 1.0e19);
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
        select(1.0e19, nearest, nearest < 1.0),
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

fn layer_color(layer: vec2<u32>) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(layer.x), unpack2x16float(layer.y));
}
