@group(4) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(4) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(4) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(4) @binding(3) var oit_residual_color: texture_2d<f32>;
@group(4) @binding(4) var oit_residual_moment: texture_2d<f32>;

const MAXIMUM_ENTRIES: u32 = 9u;

const MINIMUM_TRANSMITTANCE: f32 = 0.003;

struct Layers {
    depth: array<f32, MAXIMUM_ENTRIES>,
    color: array<vec2<u32>, MAXIMUM_ENTRIES>,
    count: u32,
}

struct Proxy {
    alpha: f32,
    depth: f32,
    color: vec3<f32>,
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
        if (layers.count > 0u && key == oit_keys[base + slot - 1u]) {
            continue;
        }
        layers.depth[layers.count] = bitcast<f32>(~key);
        layers.color[layers.count] = oit_colors[base + slot];
        layers.count += 1u;
    }
    return layers;
}

fn residual_proxy(clip_position: vec4<f32>) -> Proxy {
    var proxy: Proxy;
    proxy.alpha = 0.0;
    proxy.depth = 1.0;
    proxy.color = vec3<f32>(0.0);
    if (frame.oit.x == 0u) {
        return proxy;
    }
    let extent = vec2<i32>(textureDimensions(oit_residual_color));
    let pixel = vec2<i32>(floor(clip_position.xy));
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= extent.x || pixel.y >= extent.y) {
        return proxy;
    }
    let accumulation = textureLoad(oit_residual_color, pixel, 0);
    let tau = accumulation.a;
    if (tau <= 0.0) {
        return proxy;
    }
    let moment = textureLoad(oit_residual_moment, pixel, 0);
    proxy.alpha = 1.0 - exp(-tau);
    proxy.depth = clamp(moment.x / tau, 0.0, 1.0);
    proxy.color = accumulation.rgb / tau;
    return proxy;
}

fn append_residual(layers: Layers, clip_position: vec4<f32>) -> Layers {
    var result = layers;
    let proxy = residual_proxy(clip_position);
    if (proxy.alpha <= 0.0) {
        return result;
    }
    var at = result.count;
    for (var index = 0u; index < result.count; index += 1u) {
        if (result.depth[index] > proxy.depth) {
            at = index;
            break;
        }
    }
    var cursor = result.count;
    while (cursor > at) {
        result.depth[cursor] = result.depth[cursor - 1u];
        result.color[cursor] = result.color[cursor - 1u];
        cursor -= 1u;
    }
    let premultiplied = vec4<f32>(proxy.color * proxy.alpha, proxy.alpha);
    result.depth[at] = proxy.depth;
    result.color[at] = vec2<u32>(
        pack2x16float(premultiplied.rg),
        pack2x16float(premultiplied.ba),
    );
    result.count = result.count + 1u;
    return result;
}

fn layer_color(layer: vec2<u32>) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(layer.x), unpack2x16float(layer.y));
}
