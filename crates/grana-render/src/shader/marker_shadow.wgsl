struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;

struct ShadowOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) center_radius: vec4<f32>,
    @location(1) local: vec2<f32>,
}

fn light_coordinate(world: vec3<f32>) -> vec3<f32> {
    return (frame.light_view * vec4<f32>(world, 1.0)).xyz;
}

fn light_basis() -> mat3x3<f32> {
    let light = normalize(frame.light.xyz);
    let hint = select(
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0),
        abs(light.y) > 0.9,
    );
    let across = normalize(cross(light, hint));
    let upward = cross(light, across);
    return mat3x3<f32>(across, upward, light);
}

@vertex
fn vs_shadow(
    @builtin(vertex_index) index: u32,
    @builtin(instance_index) instance: u32,
) -> ShadowOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = corners[index];
    let data = markers[instance];
    let basis = light_basis();
    let world = data.center_radius.xyz
        + (basis[0] * local.x + basis[1] * local.y) * data.center_radius.w;
    let coordinate = light_coordinate(world);
    var output: ShadowOutput;
    output.clip_position = vec4<f32>(
        coordinate.x * 2.0 - 1.0,
        1.0 - coordinate.y * 2.0,
        coordinate.z,
        1.0,
    );
    output.center_radius = data.center_radius;
    output.local = local;
    return output;
}

@fragment
fn fs_shadow(input: ShadowOutput) -> @builtin(frag_depth) f32 {
    let squared = dot(input.local, input.local);
    if (squared > 1.0) {
        discard;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let basis = light_basis();
    let surface = input.center_radius.xyz
        + (basis[0] * input.local.x + basis[1] * input.local.y + basis[2] * depth)
            * input.center_radius.w;
    return light_coordinate(surface).z;
}
