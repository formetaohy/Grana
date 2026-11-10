struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;
@group(1) @binding(1) var<storage, read> order: array<u32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) center_radius: vec4<f32>,
    @location(1) local: vec2<f32>,
}

@vertex
fn vs_ao(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> VertexOutput {
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
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip.xy + frame.jitter.xy * clip.w + lens_shift(clip.w), clip.zw);
    output.center_radius = vec4<f32>(center, radius);
    output.local = local;
    return output;
}

@fragment
fn fs_ao(input: VertexOutput) -> @location(0) vec4<f32> {
    let squared = dot(input.local, input.local);
    if (squared > 1.0) {
        discard;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let surface = input.center_radius.xyz
        + (frame.right.xyz * input.local.x + frame.up.xyz * input.local.y - frame.forward.xyz * depth)
            * input.center_radius.w;
    if (outside_region(surface) || outside_clip(surface)) {
        discard;
    }
    let normal = normalize(
        frame.right.xyz * input.local.x + frame.up.xyz * input.local.y - frame.forward.xyz * depth,
    );
    return vec4<f32>(normal, 1.0);
}
