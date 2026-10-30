struct Uniforms {
    uv_center: vec4<f32>,
    uv_scale: vec4<f32>,
    plane: vec4<f32>,
    axes: vec4<u32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    dims: vec4<f32>,
    box_min: vec4<f32>,
    world: vec4<f32>,
    mapping: vec4<u32>,
    channel_opacity: array<vec4<f32>, 2>,
    channel_window: array<vec4<f32>, 2>,
    channel_inverse: array<vec4<f32>, 2>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> styles: array<Style>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) object: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) height: f32,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) object: u32,
}

struct DepthOutput {
    @builtin(frag_depth) depth: f32,
}

fn plane_depth(height: f32) -> f32 {
    let plane = uniforms.plane.x;
    return (height - plane) / max(1.0 - plane, 1.0e-4);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let axis = uniforms.mapping.z;
    let u = uniforms.mapping.x;
    let v = uniforms.mapping.y;
    let uvw = (input.position - uniforms.box_min.xyz) / uniforms.world.xyz;
    let ndc = vec2<f32>(
        (uvw[u] - uniforms.uv_center.x) / uniforms.uv_scale.x,
        (uvw[v] - uniforms.uv_center.y) / uniforms.uv_scale.y,
    );
    var output: VertexOutput;
    output.clip_position = vec4<f32>(ndc, 0.5, 1.0);
    output.normal = input.normal;
    output.height = uvw[axis];
    output.uv = vec2<f32>(uvw[u], uvw[v]);
    output.object = input.object;
    return output;
}

fn hidden(input: VertexOutput) -> bool {
    if (input.uv.x < 0.0 || input.uv.x > 1.0 || input.uv.y < 0.0 || input.uv.y > 1.0) {
        return true;
    }
    if (input.height < uniforms.plane.x) {
        return true;
    }
    return styles[input.object].flags.x < 0.5;
}

@fragment
fn fs_depth(input: VertexOutput) -> DepthOutput {
    if (hidden(input)) {
        discard;
    }
    var output: DepthOutput;
    output.depth = plane_depth(input.height);
    return output;
}

struct ColorOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_main(input: VertexOutput) -> ColorOutput {
    if (hidden(input)) {
        discard;
    }
    var direction = vec3<f32>(0.0);
    direction[uniforms.mapping.z] = 1.0;
    if (dot(normalize(input.normal), direction) <= 0.0) {
        discard;
    }
    let style = styles[input.object];
    var color = style.color.rgb;
    if (style.flags.y > 0.5) {
        color = mix(color, vec3<f32>(1.0, 0.86, 0.45), 0.55);
    }
    var output: ColorOutput;
    output.color = vec4<f32>(color, 1.0);
    output.depth = plane_depth(input.height);
    return output;
}
