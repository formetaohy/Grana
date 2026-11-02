struct CutObject {
    minimum: vec4<f32>,
    extent: vec4<f32>,
    style: u32,
    reserved: array<u32, 3>,
}

struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> cuts: array<CutObject>;
@group(1) @binding(1) var<storage, read> styles: array<Style>;

const CORNERS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
);

const SELECTED: vec3<f32> = vec3<f32>(1.0, 0.86, 0.45);
const SELECTED_MIX: f32 = 0.55;
const FILL_ALPHA: f32 = 0.6;
const SELECTED_ALPHA: f32 = 0.85;

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
}

struct CutOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) height: f32,
    @location(1) world: vec3<f32>,
}

struct FillOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) style: u32,
}

struct FillFragment {
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
    var normal = vec3<f32>(0.0);
    normal[section.mapping.z] = 1.0;
    return normal;
}

fn lit(color: vec3<f32>) -> vec3<f32> {
    let normal = cut_normal();
    let diffuse = max(dot(normal, frame.light.xyz), 0.0);
    let half_vector = normalize(frame.light.xyz + normal);
    let specular = pow(max(dot(normal, half_vector), 0.0), frame.shading.w);
    return color * (frame.shading.x + frame.shading.y * diffuse)
        + vec3<f32>(frame.shading.z * specular);
}

fn plane_world(uv: vec2<f32>) -> vec3<f32> {
    var uvw = vec3<f32>(0.0);
    uvw[section.mapping.z] = section.plane.x;
    uvw[section.mapping.x] = uv.x;
    uvw[section.mapping.y] = uv.y;
    return section.box_min.xyz + uvw * section.world.xyz;
}

@vertex
fn vs_cut(input: VertexInput, @builtin(instance_index) instance: u32) -> CutOutput {
    let object = cuts[instance];
    let world = object.minimum.xyz + input.position.xyz * object.extent.xyz;
    let uvw = (world - section.box_min.xyz) / section.world.xyz;
    var output: CutOutput;
    output.clip_position = vec4<f32>(
        (uvw[section.mapping.x] - section.uv_center.x) / section.uv_scale.x + section.jitter.x,
        (uvw[section.mapping.y] - section.uv_center.y) / section.uv_scale.y + section.jitter.y,
        0.5,
        1.0,
    );
    output.height = uvw[section.mapping.z];
    output.world = world;
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
fn vs_fill(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> FillOutput {
    let corner = CORNERS[index];
    var output: FillOutput;
    output.clip_position = vec4<f32>(corner + section.jitter.xy, 0.5, 1.0);
    output.uv = section.uv_center.xy + corner * section.uv_scale.xy;
    output.style = cuts[instance].style;
    return output;
}

@fragment
fn fs_fill(input: FillOutput) -> FillFragment {
    let world = plane_world(input.uv);
    let style = styles[input.style];
    var fragment: FillFragment;
    if (outside_region(world)) {
        fragment.color = vec4<f32>(0.0);
        fragment.identity = vec4<f32>(0.0);
        return fragment;
    }
    var color = lit(style.color.rgb);
    var alpha = FILL_ALPHA;
    if (style.flags.y > 0.5) {
        color = mix(color, lit(SELECTED), SELECTED_MIX);
        alpha = SELECTED_ALPHA;
    }
    fragment.color = vec4<f32>(color * alpha, alpha);
    fragment.identity = encode_identity(input.style + 1u);
    return fragment;
}
