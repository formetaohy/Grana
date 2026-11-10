struct SectionVertex {
    position: vec4<f32>,
    color: vec4<f32>,
    pick: vec4<u32>,
}

@group(1) @binding(0) var<storage, read> vertices: array<SectionVertex>;

struct SectionOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) pick: u32,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> SectionOutput {
    let vertex = vertices[index];
    let clip = frame.view_projection * vec4<f32>(vertex.position.xyz, 1.0);
    var output: SectionOutput;
    output.clip_position = vec4<f32>(
        clip.xy + frame.jitter.xy * clip.w + lens_shift(clip.w),
        clip.zw,
    );
    output.color = vertex.color;
    output.world = vertex.position.xyz;
    output.pick = vertex.pick.x;
    return output;
}

fn inside(world: vec3<f32>) -> bool {
    return !outside_region(world) && !outside_clip(world);
}

@fragment
fn fs_main(input: SectionOutput) -> @location(0) vec4<f32> {
    if (!inside(input.world)) {
        discard;
    }
    return vec4<f32>(input.color.rgb * input.color.a, input.color.a);
}

@fragment
fn fs_pick(input: SectionOutput) -> @location(0) vec2<u32> {
    if (!inside(input.world)) {
        discard;
    }
    return vec2<u32>(1u, input.pick);
}
