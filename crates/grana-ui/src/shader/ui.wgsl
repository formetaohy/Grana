struct Uniforms {
    viewport: vec2<f32>,
    scale: f32,
    time: f32,
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

const KIND_SURFACE: u32 = 0u;
const KIND_GLASS: u32 = 1u;
const KIND_SHADOW: u32 = 2u;
const KIND_GLYPH: u32 = 3u;
const KIND_SCENE: u32 = 4u;
const KIND_SEGMENT: u32 = 5u;
const KIND_RING: u32 = 6u;

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
    let exponent = 4.0;
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

fn blurred(uv: vec2<f32>, radius: f32) -> vec3<f32> {
    let levels = f32(textureNumLevels(backdrop));
    let lod = clamp(log2(max(radius, 1.0)) - 0.5, 0.0, levels - 1.0);
    let texel = exp2(floor(lod)) / uniforms.viewport;
    let offset = texel * 0.5;
    var sum = textureSampleLevel(backdrop, backdrop_sampler, uv + vec2<f32>(-offset.x, -offset.y), lod).rgb;
    sum += textureSampleLevel(backdrop, backdrop_sampler, uv + vec2<f32>(offset.x, -offset.y), lod).rgb;
    sum += textureSampleLevel(backdrop, backdrop_sampler, uv + vec2<f32>(-offset.x, offset.y), lod).rgb;
    sum += textureSampleLevel(backdrop, backdrop_sampler, uv + vec2<f32>(offset.x, offset.y), lod).rgb;
    return sum * 0.25;
}

fn bevel_height(inset: f32, thickness: f32) -> f32 {
    if (inset <= 0.0) {
        return 0.0;
    }
    let clamped = min(inset, thickness);
    return sqrt(max(0.0, clamped * (2.0 * thickness - clamped)));
}

fn shade_glass(
    p: vec2<f32>,
    size: vec2<f32>,
    radius: vec4<f32>,
    glass: vec4<f32>,
    glass2: vec4<f32>,
    specular: f32,
    tint: vec4<f32>,
    screen_uv: vec2<f32>,
) -> vec3<f32> {
    let half = size * 0.5;
    let thickness = max(glass.w * uniforms.scale, 1.0);
    let inset = clamp(-squircle(p, half, radius), 0.0, thickness);
    let step = 1.0;
    let height = bevel_height(inset, thickness);
    let height_right = bevel_height(clamp(-squircle(p + vec2<f32>(step, 0.0), half, radius), 0.0, thickness), thickness);
    let height_left = bevel_height(clamp(-squircle(p - vec2<f32>(step, 0.0), half, radius), 0.0, thickness), thickness);
    let height_down = bevel_height(clamp(-squircle(p + vec2<f32>(0.0, step), half, radius), 0.0, thickness), thickness);
    let height_up = bevel_height(clamp(-squircle(p - vec2<f32>(0.0, step), half, radius), 0.0, thickness), thickness);
    var bend = vec2<f32>((height_right - height_left) / (2.0 * step), (height_down - height_up) / (2.0 * step));
    bend = clamp(bend, vec2<f32>(-2.5, -2.5), vec2<f32>(2.5, 2.5));

    let depth = clamp(inset / thickness, 0.0, 1.0);
    let centre = -p / max(half, vec2<f32>(1.0));
    let strength = glass.y * uniforms.scale;
    let lens = bend * strength + centre * strength * 0.08 * depth;
    let base_uv = screen_uv + lens / uniforms.viewport;

    let dispersion = glass.z * strength * 0.02;
    var color: vec3<f32>;
    if (dispersion > 0.01) {
        color = vec3<f32>(
            blurred(base_uv + bend * dispersion / uniforms.viewport, glass.x * uniforms.scale).r,
            blurred(base_uv, glass.x * uniforms.scale).g,
            blurred(base_uv - bend * dispersion / uniforms.viewport, glass.x * uniforms.scale).b,
        );
    } else {
        color = blurred(base_uv, glass.x * uniforms.scale);
    }

    let luminance = dot(color, vec3<f32>(0.299, 0.587, 0.114));
    let veil = clamp(luminance * 1.08 + 0.015, 0.0, 1.0);
    color = mix(vec3<f32>(luminance), color, glass2.x);
    color = mix(color, vec3<f32>(veil), 0.22);
    color = color * glass2.y;
    color = color * (1.0 - tint.a) + tint.rgb * tint.a;

    let normal = normalize(vec3<f32>(-bend * 0.55, 1.0));
    let light = normalize(vec3<f32>(-0.30, -0.58, 0.76));
    let halfway = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let specular_term = pow(max(dot(normal, halfway), 0.0), 90.0) * specular;

    let top = clamp(0.5 - p.y / max(size.y, 1.0), 0.0, 1.0);
    let top_weight = top * top;
    let absorption = exp(-inset * inset / 1.1);
    let ring = exp(-pow((inset - 2.6) / 2.05, 2.0));
    let rim = ring * glass2.z * (0.70 + 0.30 * top_weight) * 0.34;
    let sheen = glass2.w * smoothstep(0.0, size.y * 0.7, -p.y + half.y * 0.4) * 0.30;

    color *= 1.0 - 0.30 * absorption;
    color *= 1.0 + 0.025 * (top - 0.5);
    color += vec3<f32>(sheen * top_weight);
    color += vec3<f32>(rim * 0.97, rim * 0.99, rim);
    color += vec3<f32>(specular_term);
    return color;
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
        let half = instance.rect.zw * scale * 0.5;
        let centre = (instance.rect.xy + instance.rect.zw * 0.5) * scale;
        let distance = squircle(in.position_px - centre, half, instance.corners * scale);
        let panel_half = instance.uv.zw * scale * 0.5;
        let panel_centre = (instance.uv.xy + instance.uv.zw * 0.5) * scale;
        let panel_distance = squircle(in.position_px - panel_centre, panel_half, instance.color2 * scale);
        let outside = smoothstep(-1.5, 0.5, panel_distance);
        let sigma = max(instance.extra.x * scale, 0.5);
        let spread = max(distance, 0.0) / sigma;
        let alpha = instance.color.a * exp(-spread * spread * 1.6) * outside * clip_mask;
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

    if (kind == KIND_GLASS) {
        let screen_uv = in.position_px / uniforms.viewport;
        let color = shade_glass(p, size, radius, instance.glass, instance.glass2, instance.extra.x, instance.color, screen_uv);
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
