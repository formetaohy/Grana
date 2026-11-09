struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    narrow_first: u32,
    narrow_count: u32,
    wide_first: u32,
    wide_count: u32,
    vertex_base: u32,
    reserved_a: u32,
    reserved_b: u32,
    reserved_c: u32,
}

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
    @location(2) object: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) position: vec3<f32>,
}

fn sgn(value: f32) -> f32 {
    if (value < 0.0) {
        return -1.0;
    }
    return 1.0;
}

fn decode_normal(packed: vec2<f32>) -> vec3<f32> {
    var x = packed.x * 2.0 - 1.0;
    var y = packed.y * 2.0 - 1.0;
    let z = 1.0 - abs(x) - abs(y);
    if (z < 0.0) {
        let across = (1.0 - abs(y)) * sgn(x);
        let down = (1.0 - abs(x)) * sgn(y);
        x = across;
        y = down;
    }
    return normalize(vec3<f32>(x, y, z));
}

@vertex
fn vs_ao(input: VertexInput) -> VertexOutput {
    let object = objects[input.object];
    let position = object.minimum.xyz + input.position.xyz * object.extent.xyz;
    let clip = frame.view_projection * vec4<f32>(position, 1.0);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip.xy + frame.jitter.xy * clip.w, clip.zw);
    output.normal = decode_normal(input.normal);
    output.position = position;
    return output;
}

@fragment
fn fs_ao(input: VertexOutput) -> @location(0) vec4<f32> {
    if (outside_region(input.position) || outside_clip(input.position)) {
        discard;
    }
    return vec4<f32>(normalize(input.normal), 1.0);
}
