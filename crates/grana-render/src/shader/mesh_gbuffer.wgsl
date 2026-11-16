struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    narrow_first: u32,
    narrow_count: u32,
    wide_first: u32,
    wide_count: u32,
    vertex_base: u32,
}

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(5) @binding(0) var<storage, read> pick_ids: array<u32>;

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
    @location(2) object: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) position: vec3<f32>,
    @location(2) @interpolate(flat) object: u32,
}

struct GbufferOutput {
    @location(0) normal: vec2<f32>,
    @location(1) id: u32,
}

@vertex
fn vs_gbuffer(input: VertexInput) -> VertexOutput {
    let object = objects[input.object];
    let position = object.minimum.xyz + input.position.xyz * object.extent.xyz;
    let clip = frame.view_projection * vec4<f32>(position, 1.0);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip.xy + frame.jitter.xy * clip.w + lens_shift(clip.w), clip.zw);
    output.normal = octahedral_decode(input.normal * 2.0 - vec2<f32>(1.0));
    output.position = position;
    output.object = input.object;
    return output;
}

@fragment
fn fs_gbuffer(input: VertexOutput) -> GbufferOutput {
    if (outside_region(input.position) || outside_clip(input.position)) {
        discard;
    }
    return GbufferOutput(octahedral_encode(normalize(input.normal)), pick_ids[input.object]);
}
