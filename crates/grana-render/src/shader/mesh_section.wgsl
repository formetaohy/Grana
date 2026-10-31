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
    let plane = section.plane.x;
    return (height - plane) / max(1.0 - plane, 1.0e-4);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let axis = section.mapping.z;
    let u = section.mapping.x;
    let v = section.mapping.y;
    let uvw = (input.position - section.box_min.xyz) / section.world.xyz;
    let ndc = vec2<f32>(
        (uvw[u] - section.uv_center.x) / section.uv_scale.x,
        (uvw[v] - section.uv_center.y) / section.uv_scale.y,
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
    return input.height < section.plane.x;
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
    direction[section.mapping.z] = 1.0;
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
