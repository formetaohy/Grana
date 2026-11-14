struct MeshUniforms {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    shading: vec4<f32>,
    region_center: vec4<f32>,
    region_axis: array<vec4<f32>, 3>,
    jitter: vec4<f32>,
    light_view: mat4x4<f32>,
    light_reach: vec4<f32>,
    shadow: vec4<f32>,
    viewport: vec4<f32>,
    oit: vec4<u32>,
    clip: vec4<f32>,
    forward: vec4<f32>,
    projection: vec4<f32>,
    lens: vec4<f32>,
}

struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    narrow_first: u32,
    narrow_count: u32,
    wide_first: u32,
    wide_count: u32,
    vertex_base: u32,
}

struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
    surface: vec4<f32>,
}

@group(0) @binding(0) var<uniform> mesh: MeshUniforms;
@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<Style>;
@group(3) @binding(0) var shadow_tau: texture_3d<f32>;
@group(3) @binding(1) var shadow_sampler: sampler;
@group(3) @binding(3) var shadow_objects: texture_depth_2d;
@group(3) @binding(4) var shadow_compare: sampler_comparison;
@group(5) @binding(0) var<storage, read> pick_ids: array<u32>;
@group(7) @binding(0) var ao_map: texture_2d<f32>;

fn ambient_occlusion(pixel: vec2<f32>) -> f32 {
    let size = vec2<i32>(mesh.viewport.zw);
    let at = vec2<i32>(floor(pixel));
    if (at.x < 0 || at.y < 0 || at.x >= size.x || at.y >= size.y) {
        return 1.0;
    }
    return textureLoad(ao_map, at, 0).r;
}

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
    @location(2) object: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) position: vec3<f32>,
    @location(2) @interpolate(flat) object: u32,
}

struct ShadowOutput {
    @builtin(position) clip_position: vec4<f32>,
}

fn sgn(value: f32) -> f32 {
    if (value < 0.0) {
        return -1.0;
    }
    return 1.0;
}

fn decode_normal(packed: vec2<f32>) -> vec3<f32> {
    var x = packed.x * 2.0 - 1.0;
    var y = packed.y * 2.0 - 1.0;
    let z = 1.0 - abs(x) - abs(y);
    if (z < 0.0) {
        let across = (1.0 - abs(y)) * sgn(x);
        let down = (1.0 - abs(x)) * sgn(y);
        x = across;
        y = down;
    }
    return normalize(vec3<f32>(x, y, z));
}

fn object_position(input: VertexInput) -> vec3<f32> {
    let object = objects[input.object];
    return object.minimum.xyz + input.position.xyz * object.extent.xyz;
}

fn lens_shift(clip_w: f32) -> vec2<f32> {
    return mesh.lens.xy * (clip_w / mesh.lens.z - 1.0);
}

fn view_direction(point: vec3<f32>) -> vec3<f32> {
    if (mesh.projection.x > 0.5) {
        return -mesh.forward.xyz;
    }
    return normalize(mesh.eye.xyz - point);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let position = object_position(input);
    let clip = mesh.view_projection * vec4<f32>(position, 1.0);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(
        clip.xy + mesh.jitter.xy * clip.w + lens_shift(clip.w),
        clip.zw,
    );
    output.normal = decode_normal(input.normal);
    output.position = position;
    output.object = input.object;
    return output;
}

@vertex
fn vs_shadow(input: VertexInput) -> ShadowOutput {
    let coordinate = shadow_coordinate(mesh.light_view, object_position(input));
    var output: ShadowOutput;
    output.clip_position = vec4<f32>(
        coordinate.x * 2.0 - 1.0,
        1.0 - coordinate.y * 2.0,
        coordinate.z,
        1.0,
    );
    return output;
}

fn outside_region(point: vec3<f32>) -> bool {
    if (mesh.region_center.w <= 0.5) {
        return false;
    }
    let offset = point - mesh.region_center.xyz;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let local = dot(offset, mesh.region_axis[axis].xyz);
        if (abs(local) > mesh.region_axis[axis].w) {
            return true;
        }
    }
    return false;
}

fn is_outside(input: VertexOutput) -> bool {
    if (outside_region(input.position)) {
        return true;
    }
    if (dot(mesh.clip.xyz, mesh.clip.xyz) <= 1.0e-6) {
        return false;
    }
    return dot(mesh.clip.xyz, input.position) > mesh.clip.w;
}

fn shade(input: VertexOutput, occlusion: f32) -> vec4<f32> {
    let style = styles[input.object];
    let normal = normalize(input.normal);
    let view = view_direction(input.position);
    let lit_normal = select(normal, -normal, dot(normal, mesh.light.xyz) < 0.0);
    let facing_normal = select(normal, -normal, dot(normal, view) < 0.0);
    let sun = sun_transmittance(
        shadow_coordinate(mesh.light_view, input.position + lit_normal * mesh.light_reach.z),
        shadow_bias(mesh.light_reach, lit_normal, mesh.light.xyz),
    );
    let strength = clamp(mesh.shadow.x, 0.0, 1.0);
    let visibility = mix(1.0, sun, strength);
    var color = shade_surface(
        style.color.rgb,
        style.surface.x,
        style.surface.y,
        facing_normal,
        view,
        mesh.light.xyz,
        visibility,
        mesh.shading.x,
        mesh.shading.y,
        occlusion,
    );
    if (style.flags.y > 0.5) {
        color = mix(color, vec3<f32>(1.0, 0.86, 0.45), 0.55);
    }
    return vec4<f32>(color, style.color.a);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (is_outside(input)) {
        discard;
    }
    return shade(input, ambient_occlusion(input.clip_position.xy - mesh.viewport.xy));
}

@fragment
fn fs_oit_key(input: VertexOutput) {
    if (is_outside(input)) {
        discard;
    }
    let origin = vec2<i32>(mesh.viewport.xy);
    let size = vec2<i32>(mesh.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        discard;
    }
    oit_offer(u32(index), mesh.oit.y, oit_key(input.clip_position.z));
}

@fragment
fn fs_oit_color(input: VertexOutput) -> Residual {
    var output = oit_residual(0.0, vec3<f32>(0.0), 0.0);
    if (is_outside(input)) {
        return output;
    }
    let origin = vec2<i32>(mesh.viewport.xy);
    let size = vec2<i32>(mesh.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        return output;
    }
    let sample = shade(input, ambient_occlusion(input.clip_position.xy - mesh.viewport.xy));
    let slot = oit_slot(u32(index), mesh.oit.y, oit_key(input.clip_position.z));
    if (slot >= 0 && oit_claim(u32(index), mesh.oit.y, u32(slot))) {
        oit_write(
            u32(index),
            mesh.oit.y,
            u32(slot),
            vec4<f32>(sample.rgb * sample.a, sample.a),
        );
        return output;
    }
    return oit_residual(sample.a, sample.rgb, input.clip_position.z);
}

@fragment
fn fs_pick(input: VertexOutput) -> @location(0) vec2<u32> {
    if (is_outside(input)) {
        discard;
    }
    return vec2<u32>(1u, pick_ids[input.object]);
}
