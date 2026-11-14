struct SplatInstance {
    center: vec4<f32>,
    sigma: vec4<f32>,
}

struct SplatAppearance {
    color: vec4<f32>,
}

struct SplatSlot {
    base: vec4<u32>,
}

@group(1) @binding(0) var<storage, read> splats: array<SplatInstance>;
@group(1) @binding(1) var<storage, read> order: array<u32>;
@group(1) @binding(2) var<storage, read> appearances: array<SplatAppearance>;
@group(2) @binding(0) var<uniform> slot: SplatSlot;

const SPLAT_REACH: f32 = 3.0;
const SPLAT_MINIMUM_SIGMA: f32 = 0.5;
const SPLAT_ALPHA_FLOOR: f32 = 1.0e-3;
const SPLAT_PICK_ALPHA: f32 = 0.08;
const SPLAT_FADE: f32 = 0.25;

struct SplatVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) center: vec3<f32>,
    @location(1) @interpolate(flat) sigma: vec3<f32>,
    @location(2) @interpolate(flat) color: vec3<f32>,
    @location(3) @interpolate(flat) alpha: f32,
    @location(4) @interpolate(flat) depth: f32,
    @location(5) @interpolate(flat) index: u32,
    @location(6) @interpolate(flat) pixel: f32,
    @location(7) world: vec3<f32>,
}

struct PickFragment {
    @location(0) id: vec2<u32>,
    @builtin(frag_depth) depth: f32,
}

struct SliceFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

struct Shaded {
    alpha: f32,
    depth: f32,
}

fn splat_extent(axis: vec3<f32>, sigma: vec3<f32>) -> f32 {
    let scaled = axis * sigma;
    return sqrt(dot(scaled, scaled));
}

fn splat_sigma(input: SplatVertex) -> vec3<f32> {
    return max(input.sigma, vec3<f32>(SPLAT_MINIMUM_SIGMA * input.pixel));
}

fn splat_ray(world: vec3<f32>) -> CameraRay {
    if (orthographic()) {
        return CameraRay(world, frame.forward.xyz);
    }
    let origin = lens_origin();
    return CameraRay(origin, normalize(world - origin));
}

fn splat_transmission(point: vec3<f32>, sigma: vec3<f32>) -> f32 {
    let widest = max(sigma.x, max(sigma.y, sigma.z));
    var transmission = 1.0;
    if (clip_active()) {
        let normal = frame.clip.xyz;
        let width = sqrt(dot(normal * normal, sigma * sigma));
        let signed = (dot(normal, point) - frame.clip.w) / max(width, 1.0e-7);
        transmission *= clamp(0.5 - SPLAT_FADE * signed, 0.0, 1.0);
    }
    if (region_active()) {
        let local = region_local(point);
        let half = region_half();
        let beyond = max(
            abs(local.x) - half.x,
            max(abs(local.y) - half.y, abs(local.z) - half.z),
        );
        transmission *= clamp(0.5 - SPLAT_FADE * beyond / max(widest, 1.0e-7), 0.0, 1.0);
    }
    return transmission;
}

fn splat_distance(center: vec3<f32>, sigma: vec3<f32>, origin: vec3<f32>, direction: vec3<f32>) -> f32 {
    let inverse = 1.0 / (sigma * sigma);
    let rate = dot(direction * direction, inverse);
    let offset = origin - center;
    let moment = dot(offset * direction, inverse);
    let travel = -moment / max(rate, 1.0e-12);
    let delta = offset + direction * travel;
    return dot(delta * delta, inverse);
}

fn splat_alpha(input: SplatVertex, origin: vec3<f32>, direction: vec3<f32>) -> f32 {
    let sigma = splat_sigma(input);
    let squared = splat_distance(input.center, sigma, origin, direction);
    if (squared >= SPLAT_REACH * SPLAT_REACH) {
        return 0.0;
    }
    let peak = input.alpha * splat_transmission(input.center, sigma);
    if (peak <= SPLAT_ALPHA_FLOOR) {
        return 0.0;
    }
    let tau = -log(max(1.0 - peak, SPLAT_ALPHA_FLOOR));
    return 1.0 - exp(-tau * exp(-0.5 * squared));
}

fn splat_slice(input: SplatVertex) -> Shaded {
    let ray = splat_ray(input.world);
    let normal = frame.slice.xyz;
    let rate = dot(normal, ray.direction);
    if (abs(rate) < 1.0e-9) {
        discard;
    }
    let travel = (frame.slice.w - dot(normal, ray.origin)) / rate;
    if (travel <= 0.0 && !orthographic()) {
        discard;
    }
    let point = ray.origin + ray.direction * travel;
    let sigma = splat_sigma(input);
    let inverse = 1.0 / (sigma * sigma);
    let offset = point - input.center;
    let squared = dot(offset * offset, inverse);
    if (squared >= SPLAT_REACH * SPLAT_REACH) {
        discard;
    }
    let peak = input.alpha * splat_transmission(point, sigma);
    let alpha = peak * exp(-0.5 * squared);
    if (alpha <= SPLAT_ALPHA_FLOOR) {
        discard;
    }
    let clip = frame.view_projection * vec4<f32>(point, 1.0);
    var output: Shaded;
    output.alpha = alpha;
    output.depth = clip.z / clip.w;
    return output;
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> SplatVertex {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = corners[index];
    let slot = order[instance];
    let data = splats[slot];
    let look = appearances[slot];
    let center = data.center.xyz;
    let sigma = max(data.sigma.xyz, vec3<f32>(1.0e-7));
    let clip_center = frame.view_projection * vec4<f32>(center, 1.0);
    let world_per_pixel = clip_center.w / max(frame.march.w, 1.0e-6);
    let floor = SPLAT_REACH * SPLAT_MINIMUM_SIGMA * world_per_pixel;
    let extent = vec2<f32>(
        max(SPLAT_REACH * splat_extent(frame.right.xyz, sigma), floor),
        max(SPLAT_REACH * splat_extent(frame.up.xyz, sigma), floor),
    );
    let world = center + frame.right.xyz * local.x * extent.x + frame.up.xyz * local.y * extent.y;
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    var output: SplatVertex;
    output.clip_position = vec4<f32>(clip.xy + frame.jitter.xy * clip.w + lens_shift(clip.w), clip.zw);
    output.center = center;
    output.sigma = sigma;
    output.color = look.color.rgb;
    output.alpha = look.color.a;
    output.depth = clip_center.z / clip_center.w;
    output.index = slot;
    output.pixel = world_per_pixel;
    output.world = world;
    return output;
}

@fragment
fn fs_slice(input: SplatVertex) -> SliceFragment {
    let shaded = splat_slice(input);
    var output: SliceFragment;
    output.color = vec4<f32>(input.color, shaded.alpha);
    output.depth = shaded.depth;
    return output;
}

@fragment
fn fs_oit_key(input: SplatVertex) {
    let ray = splat_ray(input.world);
    if (splat_alpha(input, ray.origin, ray.direction) <= 0.0) {
        discard;
    }
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        discard;
    }
    oit_offer(u32(index), frame.oit.y, oit_key(input.depth));
}

@fragment
fn fs_oit_color(input: SplatVertex) -> Residual {
    var output = oit_residual(0.0, vec3<f32>(0.0), 0.0);
    let ray = splat_ray(input.world);
    let alpha = splat_alpha(input, ray.origin, ray.direction);
    if (alpha <= 0.0) {
        return output;
    }
    let origin = vec2<i32>(frame.viewport.xy);
    let size = vec2<i32>(frame.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        return output;
    }
    let slot = oit_slot(u32(index), frame.oit.y, oit_key(input.depth));
    if (slot >= 0 && oit_claim(u32(index), frame.oit.y, u32(slot))) {
        oit_write(u32(index), frame.oit.y, u32(slot), vec4<f32>(input.color * alpha, alpha));
        return output;
    }
    return oit_residual(alpha, input.color, input.depth);
}

@fragment
fn fs_pick(input: SplatVertex) -> PickFragment {
    let ray = splat_ray(input.world);
    if (splat_alpha(input, ray.origin, ray.direction) < SPLAT_PICK_ALPHA) {
        discard;
    }
    var output: PickFragment;
    output.id = vec2<u32>(2u, slot.base.x + input.index + 1u);
    output.depth = input.depth;
    return output;
}

@fragment
fn fs_pick_slice(input: SplatVertex) -> PickFragment {
    let shaded = splat_slice(input);
    if (shaded.alpha < SPLAT_PICK_ALPHA) {
        discard;
    }
    var output: PickFragment;
    output.id = vec2<u32>(2u, slot.base.x + input.index + 1u);
    output.depth = shaded.depth;
    return output;
}
