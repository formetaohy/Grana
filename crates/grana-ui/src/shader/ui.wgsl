struct Uniforms {
    viewport: vec2<f32>,
    scale: f32,
    time: f32,
    pointer: vec2<f32>,
    padding: vec2<f32>,
};

struct Instance {
    rect: vec4<f32>,
    corners: vec4<f32>,
    color: vec4<f32>,
    color2: vec4<f32>,
    glass: vec4<f32>,
    glass2: vec4<f32>,
    uv: vec4<f32>,
    clip: vec4<f32>,
    clip_corners: vec4<f32>,
    tag: vec4<u32>,
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> instances: array<Instance>;
@group(0) @binding(2) var backdrop: texture_2d<f32>;
@group(0) @binding(3) var backdrop_sampler: sampler;
@group(0) @binding(4) var scene: texture_2d<f32>;
@group(0) @binding(5) var scene_sampler: sampler;
@group(0) @binding(6) var atlas: texture_2d<f32>;
@group(0) @binding(7) var atlas_sampler: sampler;
@group(0) @binding(8) var glass_map: texture_2d<f32>;
@group(0) @binding(9) var glass_map_sampler: sampler;
@group(0) @binding(10) var sharp_backdrop: texture_2d<f32>;

const KIND_SURFACE: u32 = 0u;
const KIND_GLASS: u32 = 1u;
const KIND_SHADOW: u32 = 2u;
const KIND_GLYPH: u32 = 3u;
const KIND_SCENE: u32 = 4u;
const KIND_SEGMENT: u32 = 5u;
const KIND_RING: u32 = 6u;
const KIND_AMBIENT: u32 = 7u;
const KIND_GLOW: u32 = 8u;
const KIND_TRIANGLE: u32 = 9u;

const FLAG_GRADIENT: u32 = 1u;
const FLAG_BORDER: u32 = 2u;

const MAP_SIZE: f32 = 256.0;
const RING: f32 = 1.5;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) position_px: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) index: u32,
};

var<private> QUAD: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

fn ndc(position_px: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(
        position_px.x / uniforms.viewport.x * 2.0 - 1.0,
        1.0 - position_px.y / uniforms.viewport.y * 2.0,
        0.0,
        1.0,
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> VertexOut {
    let instance = instances[index];
    let kind = instance.tag.x;
    let corner = QUAD[vertex];
    var position_px: vec2<f32>;
    var uv: vec2<f32>;
    if (kind == KIND_SEGMENT) {
        let a = instance.rect.xy;
        let b = instance.rect.zw;
        let direction = normalize(b - a + vec2<f32>(0.0001, 0.0));
        let normal = vec2<f32>(-direction.y, direction.x) * instance.extra.x * 0.5;
        let side = corner.x * 2.0 - 1.0;
        let along = corner.y;
        position_px = mix(a, b, along) + normal * side;
        position_px = position_px * uniforms.scale;
        uv = vec2<f32>(along, side * 0.5 + 0.5);
    } else if (kind == KIND_RING) {
        let center = instance.rect.xy;
        let extent = instance.rect.z + instance.rect.w;
        position_px = (center + (corner * 2.0 - 1.0) * extent) * uniforms.scale;
        uv = corner;
    } else {
        position_px = (instance.rect.xy + corner * instance.rect.zw) * uniforms.scale;
        uv = mix(instance.uv.xy, instance.uv.zw, corner);
    }
    var out: VertexOut;
    out.position = ndc(position_px);
    out.position_px = position_px;
    out.uv = uv;
    out.index = index;
    return out;
}

fn squircle(p: vec2<f32>, half: vec2<f32>, radius: vec4<f32>) -> f32 {
    var r = radius.x;
    var quadrant = p;
    if (p.x > 0.0 && p.y < 0.0) {
        r = radius.y;
    } else if (p.x > 0.0 && p.y >= 0.0) {
        r = radius.z;
    } else if (p.x <= 0.0 && p.y >= 0.0) {
        r = radius.w;
    } else {
        r = radius.x;
    }
    r = min(r, min(half.x, half.y));
    let q = abs(quadrant) - half + vec2<f32>(r);
    if (q.x <= 0.0 || q.y <= 0.0) {
        return max(q.x, q.y) - r;
    }
    let ratio = clamp(r / max(min(half.x, half.y), 0.001), 0.0, 1.0);
    let exponent = mix(2.0, 4.0, clamp((1.0 - ratio) * 2.0, 0.0, 1.0));
    let norm = pow(pow(q.x, exponent) + pow(q.y, exponent), 1.0 / exponent);
    return norm - r;
}

fn coverage(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

fn premultiplied(color: vec4<f32>, mask: f32) -> vec4<f32> {
    let alpha = clamp(color.a * mask, 0.0, 1.0);
    return vec4<f32>(color.rgb * alpha, alpha);
}

fn normal_cdf(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-1.702 * x));
}

fn backdrop_sample(local: vec2<f32>, size: vec2<f32>, margin: f32) -> vec3<f32> {
    let clamped = clamp(local, vec2<f32>(0.5), size - vec2<f32>(0.5));
    let surface = size + vec2<f32>(2.0 * margin);
    return textureSampleLevel(backdrop, backdrop_sampler, (clamped + vec2<f32>(margin)) / surface, 0.0).rgb;
}

fn sharp_sample(screen: vec2<f32>) -> vec3<f32> {
    let viewport = uniforms.viewport;
    let uv = clamp(screen / viewport, vec2<f32>(0.5) / viewport, vec2<f32>(1.0) - vec2<f32>(0.5) / viewport);
    return textureSampleLevel(sharp_backdrop, backdrop_sampler, uv, 0.0).rgb;
}

fn glass_gradient_alpha(t: f32, low: f32, high: f32, low_stop: f32, high_stop: f32) -> f32 {
    if (t <= 0.0) {
        return 0.0;
    }
    if (t < low_stop) {
        return mix(0.0, low, t / max(low_stop, 0.0001));
    }
    if (t < high_stop) {
        return mix(low, high, (t - low_stop) / max(high_stop - low_stop, 0.0001));
    }
    return mix(high, 0.0, (t - high_stop) / max(1.0 - high_stop, 0.0001));
}

fn ring_layer(
    white_low: f32,
    white_high: f32,
    gradient_t: f32,
    low_stop: f32,
    high_stop: f32,
    inset: f32,
) -> vec4<f32> {
    let a = glass_gradient_alpha(gradient_t, white_low, white_high, low_stop, high_stop);
    var rgb = vec3<f32>(a);
    var alpha = a;
    let white_hard = 0.650 * clamp(1.50 - inset, 0.0, 1.0);
    let white_soft = 0.160 * exp(-pow((inset - 1.00) / 1.50, 2.0) * 0.5);
    let black_soft = 0.350 * exp(-pow((inset - 1.00) / 2.00, 2.0) * 0.5);
    let white = clamp(white_hard + white_soft, 0.0, 1.0);
    rgb = rgb * (1.0 - white) + white;
    alpha = alpha * (1.0 - white) + white;
    rgb = rgb * (1.0 - black_soft);
    alpha = alpha * (1.0 - black_soft) + black_soft;
    return vec4<f32>(rgb, alpha);
}

const PI: f32 = 3.14159265359;
const STUDIO_THICKNESS: f32 = 20.0;
const STUDIO_DISTANCE: f32 = 64.0;
const STUDIO_IOR: f32 = 1.4;
const STUDIO_DISPERSION: f32 = 7.0;
const STUDIO_FRESNEL_RANGE: f32 = 30.0;
const STUDIO_FRESNEL_HARDNESS: f32 = 0.20;
const STUDIO_FRESNEL_FACTOR: f32 = 0.20;
const STUDIO_GLARE_RANGE: f32 = 30.0;
const STUDIO_GLARE_HARDNESS: f32 = 0.20;
const STUDIO_GLARE_FACTOR: f32 = 0.90;
const STUDIO_GLARE_CONVERGENCE: f32 = 0.50;
const STUDIO_GLARE_OPPOSITE: f32 = 0.80;
const STUDIO_GLARE_ANGLE: f32 = -PI * 0.25;
const STUDIO_EDGE_GAIN: f32 = 8.0;

fn studio_glass(
    p: vec2<f32>,
    size: vec2<f32>,
    half: vec2<f32>,
    radius: vec4<f32>,
    origin: vec2<f32>,
    sdf: f32,
    tint: vec4<f32>,
    specular: f32,
    haze: f32,
    margin: f32,
) -> vec3<f32> {
    let local = p + size * 0.5;
    let base = origin + local;
    let inside = max(-sdf, 0.0);
    let thickness = STUDIO_THICKNESS * uniforms.scale;
    let ratio = 1.0 - clamp(inside / thickness, 0.0, 1.0);
    let theta_i = asin(clamp(ratio * ratio, 0.0, 1.0));
    let theta_t = asin(clamp(sin(theta_i) / STUDIO_IOR, 0.0, 1.0));
    let edge_factor = select(0.0, max(-tan(theta_t - theta_i), 0.0), inside < thickness);

    let epsilon = max(uniforms.scale, 0.75);
    let normal_x = squircle(p + vec2<f32>(epsilon, 0.0), half, radius)
        - squircle(p - vec2<f32>(epsilon, 0.0), half, radius);
    let normal_y = squircle(p + vec2<f32>(0.0, epsilon), half, radius)
        - squircle(p - vec2<f32>(0.0, epsilon), half, radius);
    let normal = normalize(vec2<f32>(normal_x, normal_y) + vec2<f32>(0.0001, 0.0001));

    let offset = -normal * edge_factor * STUDIO_DISTANCE * uniforms.scale;
    let spread = 0.02 * STUDIO_DISPERSION;
    let refracted = vec3<f32>(
        sharp_sample(base + offset * (1.0 + spread)).r,
        sharp_sample(base + offset).g,
        sharp_sample(base + offset * (1.0 - spread)).b,
    );
    let interior = mix(
        sharp_sample(base),
        backdrop_sample(local, size, margin),
        clamp(haze, 0.0, 1.0),
    );
    var color = mix(
        mix(interior, tint.rgb, tint.a),
        mix(refracted, tint.rgb, tint.a * 0.25),
        clamp(edge_factor * STUDIO_EDGE_GAIN, 0.0, 1.0),
    );

    let fresnel_geo = clamp(
        pow(
            max(
                1.0 - inside / 1500.0 * pow(500.0 / STUDIO_FRESNEL_RANGE, 2.0)
                    + STUDIO_FRESNEL_HARDNESS,
                0.0,
            ),
            5.0,
        ),
        0.0,
        1.0,
    );
    color = mix(
        color,
        vec3<f32>(1.0),
        fresnel_geo * STUDIO_FRESNEL_FACTOR * 0.7 * specular,
    );

    let glare_geo = clamp(
        pow(
            max(
                1.0 - inside / 1500.0 * pow(500.0 / STUDIO_GLARE_RANGE, 2.0)
                    + STUDIO_GLARE_HARDNESS,
                0.0,
            ),
            5.0,
        ),
        0.0,
        1.0,
    );
    let glare_angle = (atan2(normal.y, normal.x) - PI * 0.25 + STUDIO_GLARE_ANGLE) * 2.0;
    let far = select(
        0.0,
        1.0,
        (glare_angle > PI * 1.5 && glare_angle < PI * 3.5) || glare_angle < -PI * 0.5,
    );
    let side = select(1.2, 1.2 * STUDIO_GLARE_OPPOSITE, far > 0.5);
    let raw = clamp(
        (0.5 + sin(glare_angle) * 0.5) * side * STUDIO_GLARE_FACTOR,
        0.0,
        1.0,
    );
    let glare = clamp(
        pow(raw, 0.1 + STUDIO_GLARE_CONVERGENCE * 2.0),
        0.0,
        1.0,
    ) * glare_geo;
    color = mix(color, vec3<f32>(1.0), clamp(glare * specular, 0.0, 1.0));
    return color;
}

fn shade_glass(
    p: vec2<f32>,
    size: vec2<f32>,
    radius: vec4<f32>,
    origin: vec2<f32>,
    glass: vec4<f32>,
    tint: vec4<f32>,
    rim_strength: f32,
    refraction: f32,
    specular: f32,
    haze: f32,
    margin: f32,
) -> vec3<f32> {
    let displacement = glass.x * uniforms.scale;
    let aberration = glass.y;
    let map_scale = max(size.x, size.y) / MAP_SIZE;
    let draw = (size - vec2<f32>(MAP_SIZE) * map_scale) * 0.5;
    let local = p + size * 0.5;
    let m = textureSampleLevel(glass_map, glass_map_sampler, (local - draw) / map_scale / MAP_SIZE, 0.0).rgb;
    let shift = vec2<f32>((0.5 - m.r) * displacement, (0.5 - m.b) * displacement);

    let half = size * 0.5;
    let sdf = squircle(p, half, radius);
    let inset = abs(sdf);
    if (refraction > 0.0) {
        return studio_glass(
            p,
            size,
            half,
            radius,
            origin,
            sdf,
            tint,
            specular,
            haze,
            margin,
        );
    }
    let lens = clamp(min(size.x, size.y) * 0.11, 14.0 * uniforms.scale, 44.0 * uniforms.scale);
    let edge = 1.0 - smoothstep(0.0, lens, inset);

    let frost_red = backdrop_sample(local + shift, size, margin);
    let frost_green = backdrop_sample(
        local + shift * (1.0 + aberration * 0.05),
        size,
        margin,
    );
    let frost_blue = backdrop_sample(
        local + shift * (1.0 + aberration * 0.10),
        size,
        margin,
    );
    let frost = mix(vec3<f32>(frost_red.r, frost_green.g, frost_blue.b), tint.rgb, tint.a);

    let base = origin + local;
    let refracted = vec3<f32>(
        sharp_sample(base + shift).r,
        sharp_sample(base + shift * (1.0 - aberration * 0.04)).g,
        sharp_sample(base + shift * (1.0 - aberration * 0.08)).b,
    );
    let refracted_tinted = mix(refracted, tint.rgb, tint.a * 0.35);
    var color = mix(frost, refracted_tinted, clamp(edge * refraction, 0.0, 1.0));

    let pointer = uniforms.pointer;
    let inside = all(pointer >= origin) && all(pointer <= origin + size);
    let offset = select(vec2<f32>(0.0), (pointer - (origin + size * 0.5)) / max(size, vec2<f32>(1.0)) * 100.0, inside);
    let variation_x = abs(offset.x);

    let angle = (135.0 + offset.x * 1.2) * 0.017453292;
    let direction = vec2<f32>(sin(angle), cos(angle));
    let gradient_length = abs(size.x * sin(angle)) + abs(size.y * cos(angle));
    let t = clamp(dot(p, direction) / max(gradient_length, 0.0001) + 0.5, 0.0, 1.0);
    let low_stop = clamp(33.0 + offset.y * 0.3, 10.0, 90.0) / 100.0;
    let high_stop = clamp(66.0 + offset.y * 0.4, 10.0, 90.0) / 100.0;
    let inset_css = inset / max(uniforms.scale, 0.0001);
    let first = ring_layer(0.120 + variation_x * 0.008, 0.400 + variation_x * 0.012, t, low_stop, high_stop, inset_css);
    let second = ring_layer(0.320 + variation_x * 0.008, 0.600 + variation_x * 0.012, t, low_stop, high_stop, inset_css);
    let ring_rgb = first.rgb * 0.2;
    var overlaid = clamp(color + ring_rgb - color * ring_rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (second.a > 0.0001) {
        let source = second.rgb / second.a;
        let low = 2.0 * overlaid * source;
        let high = 1.0 - 2.0 * (1.0 - overlaid) * (1.0 - source);
        let blended = select(high, low, overlaid < vec3<f32>(0.5));
        overlaid = mix(overlaid, clamp(blended, vec3<f32>(0.0), vec3<f32>(1.0)), second.a);
    }
    let ring = clamp((1.3551 + 0.5) * uniforms.scale - inset, 0.0, 1.0) * rim_strength;
    return mix(color, overlaid, clamp(ring, 0.0, 1.0));
}

fn scene_tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scene, scene_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

fn ambient_color(in: VertexOut, instance: Instance) -> vec3<f32> {
    let size = max(instance.rect.zw, vec2<f32>(1.0));
    let step = (instance.uv.zw - instance.uv.xy) / size * instance.extra.x;
    let uv = in.uv;
    var color = scene_tap(uv) * 0.24;
    color += scene_tap(uv + vec2<f32>(step.x, 0.0)) * 0.11;
    color += scene_tap(uv - vec2<f32>(step.x, 0.0)) * 0.11;
    color += scene_tap(uv + vec2<f32>(0.0, step.y)) * 0.11;
    color += scene_tap(uv - vec2<f32>(0.0, step.y)) * 0.11;
    color += scene_tap(uv + step * 0.7) * 0.08;
    color += scene_tap(uv - step * 0.7) * 0.08;
    color += scene_tap(uv + vec2<f32>(step.x, -step.y) * 0.7) * 0.08;
    color += scene_tap(uv + vec2<f32>(-step.x, step.y) * 0.7) * 0.08;
    let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    color = mix(vec3<f32>(luminance), color, instance.extra.y);
    color *= instance.extra.z;
    return mix(color, instance.color.rgb, instance.color.a);
}

fn cross2(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

fn triangle_coverage(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, point: vec2<f32>) -> f32 {
    let area = cross2(b - a, c - a);
    if (abs(area) < 0.00001) {
        return 0.0;
    }
    let sign = select(-1.0, 1.0, area > 0.0);
    let ab = sign * cross2(b - a, point - a) / max(length(b - a), 0.0001);
    let bc = sign * cross2(c - b, point - b) / max(length(c - b), 0.0001);
    let ca = sign * cross2(a - c, point - c) / max(length(a - c), 0.0001);
    return clamp(min(ab, min(bc, ca)) + 0.5, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let instance = instances[in.index];
    let kind = instance.tag.x;
    let flags = instance.tag.y;
    let scale = uniforms.scale;

    let clip_centre = (instance.clip.xy + instance.clip.zw * 0.5) * scale;
    let clip_half = instance.clip.zw * scale * 0.5;
    let clip_distance = squircle(in.position_px - clip_centre, clip_half, instance.clip_corners * scale);
    if (clip_distance > 0.5) {
        discard;
    }
    let clip_mask = coverage(clip_distance);

    if (kind == KIND_GLYPH) {
        var alpha = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).r;
        alpha = pow(alpha, instance.extra.x);
        return premultiplied(instance.color, alpha * clip_mask);
    }

    if (kind == KIND_SHADOW) {
        let shape_half = instance.uv.zw * scale * 0.5;
        let shape_centre = (instance.uv.xy + instance.uv.zw * 0.5) * scale;
        let distance = squircle(in.position_px - shape_centre, shape_half, instance.color2 * scale);
        let sigma = max(instance.extra.x * scale, 0.75);
        let coverage = normal_cdf(-distance / sigma);
        let alpha = instance.color.a * coverage * clip_mask;
        return vec4<f32>(instance.color.rgb * alpha, alpha);
    }

    if (kind == KIND_SEGMENT) {
        let a = instance.rect.xy * scale;
        let b = instance.rect.zw * scale;
        let radius = instance.extra.x * scale * 0.5;
        let point = in.position_px;
        let along = b - a;
        let length_squared = max(dot(along, along), 0.0001);
        let t = clamp(dot(point - a, along) / length_squared, 0.0, 1.0);
        let distance = length(point - (a + along * t)) - radius;
        if (distance > 0.5) {
            discard;
        }
        let mask = coverage(distance) * clip_mask;
        let color = mix(instance.color, instance.color2, t);
        return premultiplied(color, mask);
    }

    if (kind == KIND_RING) {
        let centre = instance.rect.xy * scale;
        let radius = instance.rect.z * scale;
        let thickness = instance.rect.w * scale;
        let offset = in.position_px - centre;
        let distance = abs(length(offset) - radius) - thickness * 0.5;
        if (distance > 0.5) {
            discard;
        }
        let angle = atan2(offset.y, offset.x);
        let start = instance.extra.x;
        let sweep = instance.extra.y;
        var phase = angle - start;
        while (phase < 0.0) {
            phase += 6.28318530718;
        }
        if (phase > sweep) {
            discard;
        }
        return premultiplied(instance.color, coverage(distance) * clip_mask);
    }

    if (kind == KIND_TRIANGLE) {
        let a = instance.uv.xy * scale;
        let b = instance.uv.zw * scale;
        let c = instance.extra.xy * scale;
        let coverage = triangle_coverage(a, b, c, in.position_px);
        if (coverage <= 0.0) {
            discard;
        }
        return premultiplied(instance.color, coverage * clip_mask);
    }

    let size = instance.rect.zw * scale;
    let centre = (instance.rect.xy + instance.rect.zw * 0.5) * scale;
    let p = in.position_px - centre;
    let half = size * 0.5;
    let radius = instance.corners * scale;
    let distance = squircle(p, half, radius);
    if (distance > 0.5) {
        discard;
    }
    let mask = coverage(distance) * clip_mask;

    if (kind == KIND_AMBIENT) {
        let alpha = clamp(instance.extra.w, 0.0, 1.0) * mask;
        return vec4<f32>(ambient_color(in, instance) * alpha, alpha);
    }

    if (kind == KIND_GLOW) {
        let radial = length(p / max(half, vec2<f32>(1.0)));
        let falloff = mix(2.0, 8.0, instance.extra.x);
        let alpha = pow(clamp(1.0 - radial, 0.0, 1.0), falloff);
        return premultiplied(instance.color, alpha * mask);
    }

    if (kind == KIND_SCENE) {
        let color = textureSampleLevel(scene, scene_sampler, in.uv, 0.0);
        return vec4<f32>(color.rgb * mask, mask);
    }

    if (kind == KIND_GLASS) {
        let origin = centre - half;
        let color = shade_glass(
            p,
            size,
            radius,
            origin,
            instance.glass,
            instance.color,
            instance.glass.w,
            instance.glass2.y,
            instance.glass2.z,
            instance.glass2.x,
            instance.extra.x * uniforms.scale,
        );
        return vec4<f32>(color * mask, mask);
    }

    var color = instance.color;
    if ((flags & FLAG_GRADIENT) != 0u) {
        let t = clamp((in.position_px.y - (instance.rect.y * scale)) / max(size.y, 1.0), 0.0, 1.0);
        color = mix(color, instance.color2, t);
    }
    var rgb = color.rgb;
    var alpha = color.a;
    if ((flags & FLAG_BORDER) != 0u) {
        let width = max(instance.extra.x * scale, 0.5);
        let depth = -distance;
        let band = smoothstep(width + 0.5, width - 0.5, depth) * smoothstep(-0.5, 0.5, depth);
        let border = instance.color2;
        let fill = premultiplied(vec4<f32>(rgb, alpha), mask);
        let ink = band * border.a * mask;
        return fill * (1.0 - ink) + vec4<f32>(border.rgb * ink, ink);
    }
    let result = premultiplied(vec4<f32>(rgb, alpha), mask);
    return result;
}
