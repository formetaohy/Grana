struct SectionStyle {
    color: vec4<f32>,
    flags: vec4<f32>,
}

struct SectionPlane {
    right: vec4<f32>,
    up: vec4<f32>,
    origin: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> corners: array<vec2<f32>>;
@group(1) @binding(1) var<storage, read> quad_styles: array<u32>;
@group(2) @binding(0) var<uniform> section: SectionPlane;
@group(2) @binding(1) var<storage, read> styles: array<SectionStyle>;

struct SectionOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) pick: u32,
    @location(3) @interpolate(flat) visible: f32,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> SectionOutput {
    let corner = corners[index];
    let style_index = quad_styles[index / 4u];
    let style = styles[style_index];
    let world = section.origin.xyz + section.right.xyz * corner.x + section.up.xyz * corner.y;
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    var output: SectionOutput;
    output.clip_position = vec4<f32>(
        clip.xy + frame.jitter.xy * clip.w + lens_shift(clip.w),
        clip.zw,
    );
    output.color = style.color;
    output.world = world;
    output.pick = style_index + 1u;
    output.visible = style.flags.x;
    return output;
}

fn inside(world: vec3<f32>) -> bool {
    return !outside_region(world) && !outside_clip(world);
}

fn painted(input: SectionOutput) -> bool {
    return inside(input.world) && input.visible >= 0.5 && input.color.a > 0.0;
}

@fragment
fn fs_main(input: SectionOutput) -> @location(0) vec4<f32> {
    if (!painted(input)) {
        discard;
    }
    return vec4<f32>(input.color.rgb * input.color.a, input.color.a);
}

@fragment
fn fs_pick(input: SectionOutput) -> @location(0) vec2<u32> {
    if (!painted(input)) {
        discard;
    }
    return vec2<u32>(1u, input.pick);
}
