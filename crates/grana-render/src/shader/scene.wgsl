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
    dims: vec4<f32>,
    grids: vec4<u32>,
    origin: vec4<f32>,
    axes: array<vec4<f32>, 3>,
    inverses: array<vec4<f32>, 3>,
    base: vec4<u32>,
    projection: vec4<f32>,
    lens: vec4<f32>,
    sampling: vec4<f32>,
    slice: vec4<f32>,
    slice_view: vec4<f32>,
    occupancy: vec4<f32>,
    bounds: vec4<u32>,
    packs: vec4<u32>,
}

struct ChannelData {
    opacity: f32,
    start: f32,
    inverse: f32,
    lut_row: f32,
    threshold: f32,
    ceiling: f32,
    mode: f32,
    nearest: f32,
}

@group(0) @binding(0) var<uniform> frame: FrameData;
@group(0) @binding(2) var<storage, read> channels: array<ChannelData>;
@group(0) @binding(4) var lut_sampler: sampler;
@group(0) @binding(5) var extinction_lut: texture_2d<f32>;

const MINIMUM_TRANSMITTANCE: f32 = 0.003;

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
    return frame.eye.xyz
        + frame.right.xyz * (frame.lens.x * frame.projection.z)
        + frame.up.xyz * (frame.lens.y * frame.projection.w);
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

fn clip_depth(world: vec3<f32>) -> f32 {
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    return clamp(clip.z / clip.w, 0.0, 1.0);
}

fn lens_shift(clip_w: f32) -> vec2<f32> {
    return frame.lens.xy * (clip_w / frame.lens.z - 1.0);
}

fn pixel_world(distance: f32) -> f32 {
    return select(distance, 1.0, orthographic()) / max(frame.sampling.x, 1.0e-6);
}

fn local_step(t: f32) -> f32 {
    return max(frame.march.x, 0.5 * frame.jitter.w * pixel_world(t));
}

fn volume_origin() -> vec3<f32> {
    return frame.origin.xyz;
}

fn volume_axis(axis: u32) -> vec3<f32> {
    return frame.axes[axis].xyz;
}

fn volume_inverse(axis: u32) -> vec3<f32> {
    return frame.inverses[axis].xyz;
}

fn volume_world(uvw: vec3<f32>) -> vec3<f32> {
    let dims = frame.dims.xyz;
    return volume_origin()
        + volume_axis(0u) * (uvw.x * dims.x)
        + volume_axis(1u) * (uvw.y * dims.y)
        + volume_axis(2u) * (uvw.z * dims.z);
}

fn volume_local(point: vec3<f32>) -> vec3<f32> {
    let offset = point - volume_origin();
    let voxel = vec3<f32>(
        dot(volume_inverse(0u), offset),
        dot(volume_inverse(1u), offset),
        dot(volume_inverse(2u), offset),
    );
    return voxel / frame.dims.xyz;
}

fn volume_rate(direction: vec3<f32>) -> vec3<f32> {
    let voxel = vec3<f32>(
        dot(volume_inverse(0u), direction),
        dot(volume_inverse(1u), direction),
        dot(volume_inverse(2u), direction),
    );
    return voxel / frame.dims.xyz;
}

fn volume_gradient(gradient: vec3<f32>) -> vec3<f32> {
    return gradient.x * volume_inverse(0u)
        + gradient.y * volume_inverse(1u)
        + gradient.z * volume_inverse(2u);
}

fn channel_opacity(index: u32) -> f32 {
    return channels[index].opacity;
}

fn channel_threshold(index: u32) -> f32 {
    return channels[index].threshold;
}

fn channel_ceiling(index: u32) -> f32 {
    return channels[index].ceiling;
}

fn channel_empty(index: u32, unit: f32) -> bool {
    if (channel_surface(index)) {
        return unit >= channel_threshold(index) && unit <= channel_ceiling(index);
    }
    return unit < channel_threshold(index);
}

fn channel_mode(index: u32) -> f32 {
    return channels[index].mode;
}

fn channel_maximum(index: u32) -> bool {
    let mode = channels[index].mode;
    return mode > 0.5 && mode < 1.5;
}

fn channel_isosurface(index: u32) -> bool {
    return channels[index].mode > 1.5;
}

fn channel_position(index: u32, unit: f32) -> f32 {
    let channel = channels[index];
    return clamp((unit - channel.start) * channel.inverse, 0.0, 1.0);
}

fn channel_tau(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(
        extinction_lut,
        lut_sampler,
        vec2<f32>(channel_position(index, unit), channels[index].lut_row),
        0.0,
    );
}

const PACK: u32 = 4u;

const PACK_MASK: u32 = 15u;

const CHANNEL_LIMIT: u32 = 16u;

const PACK_LIMIT: u32 = CHANNEL_LIMIT / PACK;

fn channel_active(channel: u32) -> bool {
    return (frame.packs.x & (1u << channel)) != 0u;
}

fn channel_peak(channel: u32) -> bool {
    return (frame.packs.z & (1u << channel)) != 0u;
}

fn projection() -> bool {
    return frame.packs.z != 0u;
}

fn pack_active(pack: u32) -> bool {
    return (frame.packs.y & (1u << pack)) != 0u;
}

fn channel_nearest(index: u32) -> bool {
    return channels[index].nearest > 0.5;
}

fn channel_surface(index: u32) -> bool {
    return (frame.packs.w & (1u << index)) != 0u;
}

fn pack_nearest(pack: u32) -> u32 {
    var mask = 0u;
    for (var component = 0u; component < PACK; component += 1u) {
        let channel = pack * PACK + component;
        if (channel_active(channel) && channel_nearest(channel)) {
            mask |= 1u << component;
        }
    }
    return mask;
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

fn slice_active() -> bool {
    return frame.slice_view.y > 0.5;
}

fn slice_crossing(origin: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let normal = frame.slice.xyz;
    let rate = dot(normal, direction);
    if (abs(rate) < 1.0e-9) {
        return vec4<f32>(0.0);
    }
    let distance = (frame.slice.w - dot(normal, origin)) / rate;
    if (distance <= 0.0) {
        return vec4<f32>(0.0);
    }
    let point = origin + direction * distance;
    let local = volume_local(point);
    if (any(local < vec3<f32>(0.0)) || any(local > vec3<f32>(1.0))) {
        return vec4<f32>(0.0);
    }
    if (outside_region(point) || outside_clip(point)) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(point, 1.0);
}

fn outside_slice(low: vec3<f32>, high: vec3<f32>) -> bool {
    if (!slice_active()) {
        return false;
    }
    let normal = frame.slice.xyz;
    let middle = dot(normal, 0.5 * (low + high)) - frame.slice.w;
    let reach = dot(abs(normal), 0.5 * (high - low));
    let half = frame.slice_view.x;
    return middle + reach < -half || middle - reach > half;
}
