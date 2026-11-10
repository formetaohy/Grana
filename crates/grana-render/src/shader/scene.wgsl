struct FrameData {
    view_projection: mat4x4<f32>,
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
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
    origins: array<vec4<f32>, 4>,
    axes: array<vec4<f32>, 12>,
    inverses: array<vec4<f32>, 12>,
    bases: vec4<u32>,
    bands: vec4<u32>,
    edge: vec4<f32>,
    projection: vec4<f32>,
    lens: vec4<f32>,
}

struct SectionData {
    center: vec4<f32>,
    plane: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    jitter: vec4<f32>,
    origin: vec4<f32>,
    axes: array<vec4<f32>, 3>,
    inverses: array<vec4<f32>, 3>,
    dims: vec4<f32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    channels: vec4<u32>,
    bases: vec4<u32>,
    fallback_origin: vec4<f32>,
    fallback_axes: array<vec4<f32>, 3>,
    fallback_inverses: array<vec4<f32>, 3>,
    fallback_dims: vec4<f32>,
    fallback_grid: vec4<u32>,
    projection: vec4<f32>,
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

struct CameraRay {
    origin: vec3<f32>,
    direction: vec3<f32>,
}

fn orthographic() -> bool {
    return frame.projection.x > 0.5;
}

fn unproject(ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = frame.inv_view_projection * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
}

fn lens_origin() -> vec3<f32> {
    return frame.eye.xyz + frame.right.xyz * frame.lens.x + frame.up.xyz * frame.lens.y;
}

fn camera_ray(ndc: vec2<f32>) -> CameraRay {
    if (orthographic()) {
        let origin = frame.eye.xyz
            + frame.right.xyz * (ndc.x * frame.projection.z)
            + frame.up.xyz * (ndc.y * frame.projection.w);
        return CameraRay(origin, frame.forward.xyz);
    }
    let central = normalize(unproject(vec3<f32>(ndc, 1.0)) - frame.eye.xyz);
    let origin = lens_origin();
    let focus = frame.eye.xyz + central * frame.lens.z;
    return CameraRay(origin, normalize(focus - origin));
}

fn view_direction(point: vec3<f32>) -> vec3<f32> {
    if (orthographic()) {
        return -frame.forward.xyz;
    }
    return normalize(frame.eye.xyz - point);
}

fn lens_shift(clip_w: f32) -> vec2<f32> {
    return frame.lens.xy * (clip_w / frame.lens.z - 1.0);
}

fn pixel_world(distance: f32) -> f32 {
    return select(distance, 1.0, orthographic()) / frame.march.w;
}

fn level_origin(band: u32) -> vec3<f32> {
    return frame.origins[band].xyz;
}

fn level_axis(band: u32, axis: u32) -> vec3<f32> {
    return frame.axes[band * 3u + axis].xyz;
}

fn level_inverse(band: u32, axis: u32) -> vec3<f32> {
    return frame.inverses[band * 3u + axis].xyz;
}

fn level_world(band: u32, uvw: vec3<f32>) -> vec3<f32> {
    let dims = frame.dims[band].xyz;
    return level_origin(band)
        + level_axis(band, 0u) * (uvw.x * dims.x)
        + level_axis(band, 1u) * (uvw.y * dims.y)
        + level_axis(band, 2u) * (uvw.z * dims.z);
}

fn level_local(band: u32, point: vec3<f32>) -> vec3<f32> {
    let offset = point - level_origin(band);
    let voxel = vec3<f32>(
        dot(level_inverse(band, 0u), offset),
        dot(level_inverse(band, 1u), offset),
        dot(level_inverse(band, 2u), offset),
    );
    return voxel / frame.dims[band].xyz;
}

fn level_rate(band: u32, direction: vec3<f32>) -> vec3<f32> {
    let voxel = vec3<f32>(
        dot(level_inverse(band, 0u), direction),
        dot(level_inverse(band, 1u), direction),
        dot(level_inverse(band, 2u), direction),
    );
    return voxel / frame.dims[band].xyz;
}

fn level_gradient(band: u32, gradient: vec3<f32>) -> vec3<f32> {
    return gradient.x * level_inverse(band, 0u)
        + gradient.y * level_inverse(band, 1u)
        + gradient.z * level_inverse(band, 2u);
}

fn level_extent(band: u32) -> vec3<f32> {
    let dims = frame.dims[band].xyz;
    return abs(level_axis(band, 0u)) * dims.x
        + abs(level_axis(band, 1u)) * dims.y
        + abs(level_axis(band, 2u)) * dims.z;
}

fn section_local(point: vec3<f32>) -> vec3<f32> {
    let offset = point - section.origin.xyz;
    return vec3<f32>(
        dot(section.inverses[0].xyz, offset),
        dot(section.inverses[1].xyz, offset),
        dot(section.inverses[2].xyz, offset),
    ) / section.dims.xyz;
}

fn section_world(local: vec3<f32>) -> vec3<f32> {
    return section.origin.xyz
        + section.axes[0].xyz * local.x
        + section.axes[1].xyz * local.y
        + section.axes[2].xyz * local.z;
}

fn fallback_local(point: vec3<f32>) -> vec3<f32> {
    let offset = point - section.fallback_origin.xyz;
    return vec3<f32>(
        dot(section.fallback_inverses[0].xyz, offset),
        dot(section.fallback_inverses[1].xyz, offset),
        dot(section.fallback_inverses[2].xyz, offset),
    ) / section.fallback_dims.xyz;
}

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
