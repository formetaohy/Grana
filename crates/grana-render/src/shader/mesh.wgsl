struct MeshUniforms {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    shading: vec4<f32>,
    region_min: vec4<f32>,
    region_max: vec4<f32>,
}

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

struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
}

@group(0) @binding(0) var<uniform> mesh: MeshUniforms;
@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<Style>;

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
fn vs_main(input: VertexInput) -> VertexOutput {
    let object = objects[input.object];
    let position = object.minimum.xyz + input.position.xyz * object.extent.xyz;
    var output: VertexOutput;
    output.clip_position = mesh.view_projection * vec4<f32>(position, 1.0);
    output.normal = decode_normal(input.normal);
    output.position = position;
    output.object = input.object;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (mesh.region_min.w > 0.5
        && (any(input.position < mesh.region_min.xyz) || any(input.position > mesh.region_max.xyz))) {
        discard;
    }
    let style = styles[input.object];
    var normal = normalize(input.normal);
    let view = normalize(mesh.eye.xyz - input.position);
    if (dot(normal, view) < 0.0) {
        normal = -normal;
    }
    let half_vector = normalize(mesh.light.xyz + view);
    let diffuse = max(dot(normal, mesh.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), mesh.shading.w);
    var color = style.color.rgb * (mesh.shading.x + mesh.shading.y * diffuse)
        + vec3<f32>(mesh.shading.z * specular);
    if (style.flags.y > 0.5) {
        color = mix(color, vec3<f32>(1.0, 0.86, 0.45), 0.55);
    }
    return vec4<f32>(color, 1.0);
}
