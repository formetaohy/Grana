struct FrameData {
    view_projection: mat4x4<f32>,
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    region_center: vec4<f32>,
    region_axis: array<vec4<f32>, 3>,
    atlas: vec4<u32>,
    shading: vec4<f32>,
    march: vec4<f32>,
    viewport: vec4<f32>,
    jitter: vec4<f32>,
    planes: array<vec4<f32>, 6>,
    light_view: mat4x4<f32>,
    light_inverse: mat4x4<f32>,
    light_reach: vec4<f32>,
    oit: vec4<u32>,
    clip: vec4<f32>,
    tag: vec4<u32>,
    dims: array<vec4<f32>, 4>,
    grids: array<vec4<u32>, 4>,
    mins: array<vec4<f32>, 4>,
    sizes: array<vec4<f32>, 4>,
    bases: vec4<u32>,
    bands: vec4<u32>,
    edge: vec4<f32>,
}

struct SectionData {
    center: vec4<f32>,
    plane: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    jitter: vec4<f32>,
    box_min: vec4<f32>,
    world: vec4<f32>,
    dims: vec4<f32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    axes: vec4<u32>,
    bases: vec4<u32>,
    fallback_dims: vec4<f32>,
    fallback_grid: vec4<u32>,
    fallback_min: vec4<f32>,
    fallback_world: vec4<f32>,
}

struct ChannelData {
    opacity: f32,
    start: f32,
    inverse: f32,
    lut_row: f32,
    threshold: f32,
}

@group(0) @binding(0) var<uniform> frame: FrameData;
@group(0) @binding(1) var<uniform> section: SectionData;
@group(0) @binding(2) var<storage, read> channels: array<ChannelData>;
@group(0) @binding(3) var lut: texture_2d<f32>;
@group(0) @binding(4) var lut_sampler: sampler;
@group(0) @binding(5) var extinction_lut: texture_2d<f32>;
@group(0) @binding(6) var section_lut: texture_2d<f32>;

const SHADING_TAU: f32 = 0.020203;

fn section_point(ndc: vec2<f32>) -> vec3<f32> {
    let across = section.center.x + ndc.x * section.right.w;
    let down = section.center.y + ndc.y * section.up.w;
    return section.right.xyz * across
        + section.up.xyz * down
        + section.plane.xyz * section.plane.w;
}

fn section_plane_distance(point: vec3<f32>) -> f32 {
    return dot(section.plane.xyz, point) - section.plane.w;
}

fn section_across(point: vec3<f32>) -> f32 {
    return dot(section.right.xyz, point) - section.center.x;
}

fn section_down(point: vec3<f32>) -> f32 {
    return dot(section.up.xyz, point) - section.center.y;
}

fn channel_opacity(index: u32) -> f32 {
    return channels[index].opacity;
}

fn channel_threshold(index: u32) -> f32 {
    return channels[index].threshold;
}

fn channel_position(index: u32, unit: f32) -> f32 {
    let channel = channels[index];
    return clamp((unit - channel.start) * channel.inverse, 0.0, 1.0);
}

fn channel_sample(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(
        lut,
        lut_sampler,
        vec2<f32>(channel_position(index, unit), channels[index].lut_row),
        0.0,
    );
}

fn channel_tau(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(
        extinction_lut,
        lut_sampler,
        vec2<f32>(channel_position(index, unit), channels[index].lut_row),
        0.0,
    );
}

fn channel_coverage(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(
        section_lut,
        lut_sampler,
        vec2<f32>(channel_position(index, unit), channels[index].lut_row),
        0.0,
    );
}

const PACK: u32 = 4u;

fn pack_active(pack: u32, count: u32) -> bool {
    var occupied = false;
    for (var component = 0u; component < PACK; component += 1u) {
        let channel = pack * PACK + component;
        if (channel >= count) {
            break;
        }
        occupied = occupied || channel_opacity(channel) > 0.0;
    }
    return occupied;
}

fn region_active() -> bool {
    return frame.region_center.w > 0.5;
}

fn region_half() -> vec3<f32> {
    return vec3<f32>(
        frame.region_axis[0].w,
        frame.region_axis[1].w,
        frame.region_axis[2].w,
    );
}

fn region_local(point: vec3<f32>) -> vec3<f32> {
    let offset = point - frame.region_center.xyz;
    return vec3<f32>(
        dot(offset, frame.region_axis[0].xyz),
        dot(offset, frame.region_axis[1].xyz),
        dot(offset, frame.region_axis[2].xyz),
    );
}

fn outside_region(point: vec3<f32>) -> bool {
    if (!region_active()) {
        return false;
    }
    return any(abs(region_local(point)) > region_half());
}

fn outside_region_bounds(low: vec3<f32>, high: vec3<f32>) -> bool {
    if (!region_active()) {
        return false;
    }
    let half = region_half();
    let middle = 0.5 * (low + high);
    let extent = 0.5 * (high - low);
    let offset = middle - frame.region_center.xyz;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let direction = frame.region_axis[axis].xyz;
        let reach = dot(extent, abs(direction));
        if (abs(dot(offset, direction)) > half[axis] + reach) {
            return true;
        }
    }
    return false;
}

fn outside_region_sphere(center: vec3<f32>, radius: f32) -> bool {
    if (!region_active()) {
        return false;
    }
    let local = region_local(center);
    let half = region_half();
    let clamped = clamp(local, -half, half);
    return distance(local, clamped) > radius;
}

fn clip_active() -> bool {
    return dot(frame.clip.xyz, frame.clip.xyz) > 1.0e-6;
}

fn outside_clip(point: vec3<f32>) -> bool {
    if (!clip_active()) {
        return false;
    }
    return dot(frame.clip.xyz, point) > frame.clip.w;
}

fn clip_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    if (!clip_active()) {
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    let rate = dot(frame.clip.xyz, direction);
    let reach = frame.clip.w - dot(frame.clip.xyz, origin);
    if (abs(rate) < 1.0e-9) {
        if (reach < 0.0) {
            return vec2<f32>(1.0, -1.0);
        }
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    let crossing = reach / rate;
    if (rate > 0.0) {
        return vec2<f32>(-1.0e20, crossing);
    }
    return vec2<f32>(crossing, 1.0e20);
}
