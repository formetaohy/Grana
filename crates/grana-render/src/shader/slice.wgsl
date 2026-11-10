struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
    surface: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> styles: array<Style>;

const SELECTED: vec3<f32> = vec3<f32>(1.0, 0.86, 0.45);
const SELECTED_MIX: f32 = 0.55;
const FILL_ALPHA: f32 = 0.6;
const SELECTED_ALPHA: f32 = 0.85;

struct FaceInput {
    @location(0) uv: vec2<f32>,
    @location(1) style: u32,
}

struct FaceOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) style: u32,
}

struct FaceFragment {
    @location(0) color: vec4<f32>,
    @location(1) identity: vec4<f32>,
}

fn encode_identity(identity: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(identity & 0xFFu) / 255.0,
        f32((identity >> 8u) & 0xFFu) / 255.0,
        f32((identity >> 16u) & 0xFFu) / 255.0,
        0.0,
    );
}

fn cut_normal() -> vec3<f32> {
    return section.plane.xyz;
}

fn lit(albedo: vec3<f32>, roughness: f32, metallic: f32, point: vec3<f32>) -> vec3<f32> {
    var normal = cut_normal();
    let view = view_direction(point);
    if (dot(normal, view) < 0.0) {
        normal = -normal;
    }
    return shade_surface(
        albedo,
        roughness,
        metallic,
        normal,
        view,
        frame.light.xyz,
        1.0,
        frame.shading.x,
        frame.shading.y,
        1.0,
    );
}

fn plane_point(uv: vec2<f32>) -> vec3<f32> {
    return section.right.xyz * uv.x
        + section.up.xyz * uv.y
        + section.plane.xyz * section.plane.w;
}

@vertex
fn vs_main(input: FaceInput) -> FaceOutput {
    var output: FaceOutput;
    output.clip_position = vec4<f32>(
        (input.uv.x - section.center.x) / section.right.w + section.jitter.x,
        (input.uv.y - section.center.y) / section.up.w + section.jitter.y,
        0.5,
        1.0,
    );
    output.uv = input.uv;
    output.style = input.style;
    return output;
}

@fragment
fn fs_main(input: FaceOutput) -> FaceFragment {
    var fragment: FaceFragment;
    let style = styles[input.style];
    let point = plane_point(input.uv);
    if (outside_region(point) || outside_clip(point)) {
        fragment.color = vec4<f32>(0.0);
        fragment.identity = vec4<f32>(0.0);
        return fragment;
    }
    var color = lit(style.color.rgb, style.surface.x, style.surface.y, point);
    var alpha = FILL_ALPHA;
    if (style.flags.y > 0.5) {
        color = mix(
            color,
            lit(SELECTED, style.surface.x, style.surface.y, point),
            SELECTED_MIX,
        );
        alpha = SELECTED_ALPHA;
    }
    fragment.color = vec4<f32>(color * alpha, alpha);
    fragment.identity = encode_identity(input.style + 1u);
    return fragment;
}

@fragment
fn fs_pick(input: FaceOutput) -> @location(0) vec2<u32> {
    let point = plane_point(input.uv);
    if (outside_region(point) || outside_clip(point)) {
        discard;
    }
    return vec2<u32>(1u, input.style + 1u);
}
