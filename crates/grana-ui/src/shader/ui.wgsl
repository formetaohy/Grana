struct Uniforms {
    viewport: vec2<f32>,
    scale: f32,
    padding: f32,
};

struct Instance {
    rect: vec4<f32>,
    corners: vec4<f32>,
    color: vec4<f32>,
    color2: vec4<f32>,
    uv: vec4<f32>,
    clip: vec4<f32>,
    clip_corners: vec4<f32>,
    tag: vec4<u32>,
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> instances: array<Instance>;
@group(0) @binding(2) var scene: texture_2d<f32>;
@group(0) @binding(3) var scene_sampler: sampler;
@group(0) @binding(4) var atlas: texture_2d<f32>;
@group(0) @binding(5) var atlas_sampler: sampler;

const KIND_SURFACE: u32 = 0u;
const KIND_SHADOW: u32 = 1u;
const KIND_GLYPH: u32 = 2u;
const KIND_SCENE: u32 = 3u;
const KIND_SEGMENT: u32 = 4u;
const KIND_RING: u32 = 5u;
const KIND_TRIANGLE: u32 = 6u;

const FLAG_GRADIENT: u32 = 1u;
const FLAG_BORDER: u32 = 2u;

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
    let quadrant = p;
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
        let outside = smoothstep(0.0, 1.5, distance);
        let alpha = instance.color.a * coverage * outside * clip_mask;
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

    if (kind == KIND_SCENE) {
        let color = textureSampleLevel(scene, scene_sampler, in.uv, 0.0);
        return vec4<f32>(color.rgb * mask, mask);
    }

    var color = instance.color;
    if ((flags & FLAG_GRADIENT) != 0u) {
        let t = clamp((in.position_px.y - (instance.rect.y * scale)) / max(size.y, 1.0), 0.0, 1.0);
        color = mix(color, instance.color2, t);
    }
    let rgb = color.rgb;
    let alpha = color.a;
    if ((flags & FLAG_BORDER) != 0u) {
        let width = max(instance.extra.x * scale, 0.5);
        let depth = -distance;
        let band = smoothstep(width + 0.5, width - 0.5, depth) * smoothstep(-0.5, 0.5, depth);
        let border = instance.color2;
        let fill = premultiplied(vec4<f32>(rgb, alpha), mask);
        let ink = band * border.a * mask;
        return fill * (1.0 - ink) + vec4<f32>(border.rgb * ink, ink);
    }
    return premultiplied(vec4<f32>(rgb, alpha), mask);
}
