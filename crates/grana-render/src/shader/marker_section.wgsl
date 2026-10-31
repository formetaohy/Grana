struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;
@group(1) @binding(1) var<storage, read> order: array<u32>;

struct MarkerOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) color_alpha: vec4<f32>,
    @location(1) local: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> MarkerOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = corners[index];
    let data = markers[order[instance]];
    let axis = section.mapping.z;
    let u = section.mapping.x;
    let v = section.mapping.y;
    let radius = data.center_radius.w;
    var uvw = (data.center_radius.xyz - section.box_min.xyz) / section.world.xyz;
    let offset = (uvw[axis] - section.plane.x) * section.world[axis];
    let cross = sqrt(max(radius * radius - offset * offset, 0.0));
    let center = vec2<f32>(
        (uvw[u] - section.uv_center.x) / section.uv_scale.x,
        (uvw[v] - section.uv_center.y) / section.uv_scale.y,
    );
    let extent = vec2<f32>(
        cross / (section.world[u] * section.uv_scale.x),
        cross / (section.world[v] * section.uv_scale.y),
    );
    var output: MarkerOutput;
    output.clip_position = vec4<f32>(center + local * extent, 0.0, 1.0);
    output.color_alpha = data.color_alpha;
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
