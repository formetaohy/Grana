struct MarkerUniforms {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    shading: vec4<f32>,
    viewport: vec4<f32>,
}

@group(0) @binding(0) var<uniform> marker: MarkerUniforms;

struct MarkerInstance {
    @location(0) center_radius: vec4<f32>,
    @location(1) color_alpha: vec4<f32>,
}

struct MarkerOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) center_radius: vec4<f32>,
    @location(1) @interpolate(flat) color_alpha: vec4<f32>,
    @location(2) local: vec2<f32>,
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
    let center = instance.center_radius.xyz;
    let clip_center = marker.view_projection * vec4<f32>(center, 1.0);
    let world_per_pixel = clip_center.w / max(marker.viewport.x, 1.0e-6);
    let radius = max(instance.center_radius.w, 1.25 * world_per_pixel);
    let world = center + (marker.right.xyz * local.x + marker.up.xyz * local.y) * radius;
    var output: MarkerOutput;
    output.clip_position = marker.view_projection * vec4<f32>(world, 1.0);
    output.center_radius = vec4<f32>(center, radius);
    output.color_alpha = instance.color_alpha;
    output.local = local;
    return output;
}

@fragment
fn fs_main(input: MarkerOutput) -> @location(0) vec4<f32> {
    let local = input.local;
    let squared = dot(local, local);
    if (squared > 1.0) {
        discard;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let radius = input.center_radius.w;
    let surface = input.center_radius.xyz
        + (marker.right.xyz * local.x + marker.up.xyz * local.y - marker.forward.xyz * depth) * radius;
    let normal = normalize(marker.right.xyz * local.x + marker.up.xyz * local.y - marker.forward.xyz * depth);
    let view = normalize(marker.eye.xyz - surface);
    let half_vector = normalize(marker.light.xyz + view);
    let diffuse = max(dot(normal, marker.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), marker.shading.w);
    let color = input.color_alpha.rgb * (marker.shading.x + marker.shading.y * diffuse)
        + vec3<f32>(marker.shading.z * specular);
    let pixel = max(fwidth(local.x), fwidth(local.y));
    let coverage = clamp((1.0 - sqrt(squared)) / max(pixel, 1.0e-6) + 0.5, 0.0, 1.0);
    return vec4<f32>(srgb_encode(color), input.color_alpha.a * coverage);
}
