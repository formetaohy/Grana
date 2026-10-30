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

struct MarkerInstance {
    @location(0) center_radius: vec4<f32>,
    @location(1) color_alpha: vec4<f32>,
}

struct MarkerOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) color_alpha: vec4<f32>,
    @location(1) local: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: MarkerInstance) -> MarkerOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = corners[index];
    let axis = uniforms.mapping.z;
    let u = uniforms.mapping.x;
    let v = uniforms.mapping.y;
    let radius = instance.center_radius.w;
    var uvw = (instance.center_radius.xyz - uniforms.box_min.xyz) / uniforms.world.xyz;
    let offset = (uvw[axis] - uniforms.plane.x) * uniforms.world[axis];
    let cross = sqrt(max(radius * radius - offset * offset, 0.0));
    let center = vec2<f32>(
        (uvw[u] - uniforms.uv_center.x) / uniforms.uv_scale.x,
        (uvw[v] - uniforms.uv_center.y) / uniforms.uv_scale.y,
    );
    let extent = vec2<f32>(
        cross / (uniforms.world[u] * uniforms.uv_scale.x),
        cross / (uniforms.world[v] * uniforms.uv_scale.y),
    );
    var output: MarkerOutput;
    output.clip_position = vec4<f32>(center + local * extent, 0.0, 1.0);
    output.color_alpha = instance.color_alpha;
    output.local = local;
    return output;
}

@fragment
fn fs_main(input: MarkerOutput) -> @location(0) vec4<f32> {
    let distance = length(input.local);
    if (distance > 1.0) {
        discard;
    }
    let width = fwidth(distance);
    let coverage = clamp((1.0 - distance) / max(width, 1.0e-6) + 0.5, 0.0, 1.0);
    let rim = 1.0 - 0.45 * smoothstep(0.72, 1.0, distance);
    return vec4<f32>(input.color_alpha.rgb * rim, input.color_alpha.a * coverage);
}
