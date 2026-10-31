struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;
@group(1) @binding(1) var<storage, read> order: array<u32>;

struct MarkerOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) center_radius: vec4<f32>,
    @location(1) @interpolate(flat) color_alpha: vec4<f32>,
    @location(2) local: vec2<f32>,
}

struct MarkerFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
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
    let center = data.center_radius.xyz;
    let clip_center = frame.view_projection * vec4<f32>(center, 1.0);
    let world_per_pixel = clip_center.w / max(frame.march.w, 1.0e-6);
    let radius = max(data.center_radius.w, 1.25 * world_per_pixel);
    let world = center + (frame.right.xyz * local.x + frame.up.xyz * local.y) * radius;
    var output: MarkerOutput;
    output.clip_position = frame.view_projection * vec4<f32>(world, 1.0);
    output.center_radius = vec4<f32>(center, radius);
    output.color_alpha = data.color_alpha;
    output.local = local;
    return output;
}

@fragment
fn fs_main(input: MarkerOutput) -> MarkerFragment {
    let local = input.local;
    let squared = dot(local, local);
    if (squared > 1.0) {
        discard;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let radius = input.center_radius.w;
    let surface = input.center_radius.xyz
        + (frame.right.xyz * local.x + frame.up.xyz * local.y - frame.forward.xyz * depth) * radius;
    let normal = normalize(
        frame.right.xyz * local.x + frame.up.xyz * local.y - frame.forward.xyz * depth,
    );
    let view = normalize(frame.eye.xyz - surface);
    let half_vector = normalize(frame.light.xyz + view);
    let diffuse = max(dot(normal, frame.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), frame.shading.w);
    let color = input.color_alpha.rgb * (frame.shading.x + frame.shading.y * diffuse)
        + vec3<f32>(frame.shading.z * specular);
    let pixel = max(fwidth(local.x), fwidth(local.y));
    let coverage = clamp((1.0 - sqrt(squared)) / max(pixel, 1.0e-6) + 0.5, 0.0, 1.0);
    let clip = frame.view_projection * vec4<f32>(surface, 1.0);
    var output: MarkerFragment;
    output.color = vec4<f32>(color, input.color_alpha.a * coverage);
    output.depth = clip.z / clip.w;
    return output;
}
