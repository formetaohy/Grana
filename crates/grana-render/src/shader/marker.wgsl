struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;
@group(1) @binding(1) var<storage, read> order: array<u32>;
@group(2) @binding(0) var ao_map: texture_2d<f32>;

fn ambient_occlusion(pixel: vec2<f32>) -> f32 {
    let size = vec2<i32>(frame.viewport.zw);
    let at = vec2<i32>(floor(pixel));
    if (at.x < 0 || at.y < 0 || at.x >= size.x || at.y >= size.y) {
        return 1.0;
    }
    return textureLoad(ao_map, at, 0).r;
}

const MARKER_ROUGHNESS: f32 = 0.30;
const MARKER_METALLIC: f32 = 0.0;

struct MarkerOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) center_radius: vec4<f32>,
    @location(1) @interpolate(flat) color_alpha: vec4<f32>,
    @location(2) local: vec2<f32>,
    @location(3) @interpolate(flat) marker: u32,
}

struct MarkerFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

struct PickFragment {
    @location(0) id: vec2<u32>,
    @builtin(frag_depth) depth: f32,
}

struct Shaded {
    color: vec4<f32>,
    depth: f32,
    surface: vec3<f32>,
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
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    var output: MarkerOutput;
    output.clip_position = vec4<f32>(clip.xy + frame.jitter.xy * clip.w, clip.zw);
    output.center_radius = vec4<f32>(center, radius);
    output.color_alpha = data.color_alpha;
    output.local = local;
    output.marker = order[instance];
    return output;
}

fn kept(input: MarkerOutput) -> bool {
    let squared = dot(input.local, input.local);
    if (squared > 1.0) {
        return false;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let radius = input.center_radius.w;
    let surface = input.center_radius.xyz
        + (frame.right.xyz * input.local.x + frame.up.xyz * input.local.y - frame.forward.xyz * depth) * radius;
    return !outside_region(surface) && !outside_clip(surface);
}

fn shade(input: MarkerOutput, occlusion: f32) -> Shaded {
    let local = input.local;
    let squared = dot(local, local);
    if (squared > 1.0) {
        discard;
    }
    let depth = sqrt(max(0.0, 1.0 - squared));
    let radius = input.center_radius.w;
    let surface = input.center_radius.xyz
        + (frame.right.xyz * local.x + frame.up.xyz * local.y - frame.forward.xyz * depth) * radius;
    if (outside_region(surface) || outside_clip(surface)) {
        discard;
    }
    let normal = normalize(
        frame.right.xyz * local.x + frame.up.xyz * local.y - frame.forward.xyz * depth,
    );
    let view = normalize(frame.eye.xyz - surface);
    let color = shade_surface(
        input.color_alpha.rgb,
        MARKER_ROUGHNESS,
        MARKER_METALLIC,
        normal,
        view,
        frame.light.xyz,
        1.0,
        frame.shading.x,
        frame.shading.y,
        occlusion,
    );
    let pixel = max(fwidth(local.x), fwidth(local.y));
    let coverage = clamp((1.0 - sqrt(squared)) / max(pixel, 1.0e-6) + 0.5, 0.0, 1.0);
    let clip = frame.view_projection * vec4<f32>(surface, 1.0);
    var output: Shaded;
    output.color = vec4<f32>(color, input.color_alpha.a * coverage);
    output.depth = clip.z / clip.w;
    output.surface = surface;
    return output;
}

@fragment
fn fs_main(input: MarkerOutput) -> MarkerFragment {
    let shaded = shade(
        input,
        ambient_occlusion(input.clip_position.xy - frame.viewport.xy),
    );
    var output: MarkerFragment;
    output.color = shaded.color;
    output.depth = shaded.depth;
    return output;
}

fn surface_depth(input: MarkerOutput) -> f32 {
    let squared = dot(input.local, input.local);
    let depth = sqrt(max(0.0, 1.0 - squared));
    let surface = input.center_radius.xyz
        + (frame.right.xyz * input.local.x + frame.up.xyz * input.local.y - frame.forward.xyz * depth)
            * input.center_radius.w;
    let clip = frame.view_projection * vec4<f32>(surface, 1.0);
    return clip.z / clip.w;
}

@fragment
fn fs_oit_key(input: MarkerOutput) -> @location(0) u32 {
    if (!kept(input)) {
        discard;
    }
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        discard;
    }
    oit_offer(u32(index), frame.oit.y, oit_key(surface_depth(input)));
    return 1u;
}

struct TailFragment {
    @location(0) accumulate: vec4<f32>,
    @location(1) reveal: vec4<f32>,
    @location(2) depth: vec2<f32>,
}

@fragment
fn fs_oit_color(input: MarkerOutput) {
    if (!kept(input)) {
        return;
    }
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        return;
    }
    let covered = oit_covered(pixel);
    if (covered <= 0.0) {
        return;
    }
    let depth = surface_depth(input);
    let slot = oit_slot(u32(index), frame.oit.y, oit_key(depth));
    if (slot < 0) {
        return;
    }
    let shaded = shade(
        input,
        ambient_occlusion(input.clip_position.xy - frame.viewport.xy),
    );
    let premultiplied = vec4<f32>(shaded.color.rgb * shaded.color.a, shaded.color.a) * covered;
    oit_write(u32(index), frame.oit.y, u32(slot), premultiplied);
}

@fragment
fn fs_oit_tail(input: MarkerOutput) -> TailFragment {
    let scale = max(frame.oit.w, 1u);
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let pixel = vec2<i32>(floor(input.clip_position.xy * f32(scale))) + origin;
    var output: TailFragment;
    output.accumulate = vec4<f32>(0.0);
    output.reveal = vec4<f32>(0.0);
    output.depth = vec2<f32>(1.0, 1.0);
    if (!kept(input)) {
        discard;
    }
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        discard;
    }
    let covered = oit_covered(pixel);
    if (covered <= 0.0) {
        discard;
    }
    let depth = surface_depth(input);
    if (!oit_beyond(u32(index), frame.oit.y, oit_key(depth))) {
        discard;
    }
    let shaded = shade(
        input,
        ambient_occlusion(input.clip_position.xy * f32(scale)),
    );
    let alpha = shaded.color.a * covered;
    let weight = oit_weight(depth, shaded.color.a);
    output.accumulate = vec4<f32>(shaded.color.rgb * alpha * weight, alpha * weight);
    output.reveal = vec4<f32>(alpha);
    output.depth = vec2<f32>(depth, 1.0);
    return output;
}

@fragment
fn fs_pick(input: MarkerOutput) -> PickFragment {
    let shaded = shade(input, 1.0);
    var output: PickFragment;
    output.id = vec2<u32>(2u, input.marker + 1u);
    output.depth = shaded.depth;
    return output;
}
