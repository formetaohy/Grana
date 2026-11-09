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
    @location(2) @interpolate(flat) center: vec3<f32>,
    @location(3) @interpolate(flat) cross: f32,
    @location(4) @interpolate(flat) marker: u32,
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
    let radius = data.center_radius.w;
    let distance = section_plane_distance(center);
    let cross = sqrt(max(radius * radius - distance * distance, 0.0));
    let at = vec2<f32>(
        section_across(center) / section.right.w,
        section_down(center) / section.up.w,
    );
    let extent = vec2<f32>(cross / section.right.w, cross / section.up.w);
    var output: MarkerOutput;
    output.clip_position = vec4<f32>(at + local * extent + section.jitter.xy, 0.0, 1.0);
    output.color_alpha = data.color_alpha;
    output.local = local;
    output.center = center;
    output.cross = cross;
    output.marker = order[instance];
    return output;
}

fn sample_point(center: vec3<f32>, local: vec2<f32>, cross: f32) -> vec3<f32> {
    return center + section.right.xyz * (local.x * cross) + section.up.xyz * (local.y * cross);
}

@fragment
fn fs_main(input: MarkerOutput) -> @location(0) vec4<f32> {
    let distance = length(input.local);
    if (distance > 1.0) {
        discard;
    }
    let world = sample_point(input.center, input.local, input.cross);
    if (outside_region(world) || outside_clip(world)) {
        discard;
    }
    let width = fwidth(distance);
    let coverage = clamp((1.0 - distance) / max(width, 1.0e-6) + 0.5, 0.0, 1.0);
    let rim = 1.0 - 0.45 * smoothstep(0.72, 1.0, distance);
    let alpha = input.color_alpha.a * coverage;
    return vec4<f32>(input.color_alpha.rgb * rim * alpha, alpha);
}

@fragment
fn fs_pick(input: MarkerOutput) -> @location(0) vec2<u32> {
    let distance = length(input.local);
    if (distance > 1.0) {
        discard;
    }
    let width = fwidth(distance);
    let coverage = clamp((1.0 - distance) / max(width, 1.0e-6) + 0.5, 0.0, 1.0);
    if (coverage < 0.5) {
        discard;
    }
    let world = sample_point(input.center, input.local, input.cross);
    if (outside_region(world) || outside_clip(world)) {
        discard;
    }
    return vec2<u32>(2u, input.marker + 1u);
}
