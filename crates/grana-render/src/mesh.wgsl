struct Uniforms {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    shading: vec4<f32>,
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
    @location(1) position: vec3<f32>,
    @location(2) @interpolate(flat) object: u32,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = uniforms.view_projection * vec4<f32>(input.position, 1.0);
    output.normal = input.normal;
    output.position = input.position;
    output.object = input.object;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let style = styles[input.object];
    if (style.flags.x < 0.5) {
        discard;
    }
    var normal = normalize(input.normal);
    let view = normalize(uniforms.eye.xyz - input.position);
    if (dot(normal, view) < 0.0) {
        normal = -normal;
    }
    let half_vector = normalize(uniforms.light.xyz + view);
    let diffuse = max(dot(normal, uniforms.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), uniforms.shading.w);
    var color = style.color.rgb * (uniforms.shading.x + uniforms.shading.y * diffuse)
        + vec3<f32>(uniforms.shading.z * specular);
    if (style.flags.y > 0.5) {
        color = mix(color, vec3<f32>(1.0, 0.86, 0.45), 0.55);
    }
    return vec4<f32>(color, 1.0);
}
