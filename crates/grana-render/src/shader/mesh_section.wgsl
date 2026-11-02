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

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<Style>;

const CORNERS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
);

const SELECTED: vec3<f32> = vec3<f32>(1.0, 0.86, 0.45);
const SELECTED_MIX: f32 = 0.55;

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
    @location(2) object: u32,
}

struct CutOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) height: f32,
}

struct FillOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) object: u32,
}

@vertex
fn vs_cut(input: VertexInput) -> CutOutput {
    let object = objects[input.object];
    let world = object.minimum.xyz + input.position.xyz * object.extent.xyz;
    let uvw = (world - section.box_min.xyz) / section.world.xyz;
    let u = section.mapping.x;
    let v = section.mapping.y;
    var output: CutOutput;
    output.clip_position = vec4<f32>(
        (uvw[u] - section.uv_center.x) / section.uv_scale.x,
        (uvw[v] - section.uv_center.y) / section.uv_scale.y,
        0.5,
        1.0,
    );
    output.height = uvw[section.mapping.z];
    return output;
}

@fragment
fn fs_parity(input: CutOutput) -> @location(0) vec4<f32> {
    if (input.height < section.plane.x) {
        discard;
    }
    return vec4<f32>(0.0);
}

@vertex
fn vs_fill(@builtin(vertex_index) index: u32, @builtin(instance_index) object: u32) -> FillOutput {
    let corner = CORNERS[index];
    var output: FillOutput;
    output.clip_position = vec4<f32>(corner, 0.5, 1.0);
    output.uv = section.uv_center.xy + corner * section.uv_scale.xy;
    output.object = object;
    return output;
}

@fragment
fn fs_fill(input: FillOutput) -> @location(0) vec4<f32> {
    var uvw = vec3<f32>(0.0);
    uvw[section.mapping.z] = section.plane.x;
    uvw[section.mapping.x] = input.uv.x;
    uvw[section.mapping.y] = input.uv.y;
    if (outside_region(section.box_min.xyz + uvw * section.world.xyz)) {
        discard;
    }
    let style = styles[input.object];
    var color = style.color.rgb;
    if (style.flags.y > 0.5) {
        color = mix(color, SELECTED, SELECTED_MIX);
    }
    return vec4<f32>(color, 1.0);
}

@vertex
fn vs_quad(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(CORNERS[index], 0.5, 1.0);
}

@fragment
fn fs_quad() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
