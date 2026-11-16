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

fn lens_shift(clip_w: f32) -> vec2<f32> {
    return mesh.lens.xy * (clip_w / mesh.lens.z - 1.0);
}

fn view_direction(point: vec3<f32>) -> vec3<f32> {
    if (mesh.projection.x > 0.5) {
        return -mesh.forward.xyz;
    }
    return normalize(mesh.eye.xyz - point);
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

fn outside_clip_point(point: vec3<f32>) -> bool {
    if (dot(mesh.clip.xyz, mesh.clip.xyz) <= 1.0e-6) {
        return false;
    }
    return dot(mesh.clip.xyz, point) > mesh.clip.w;
}

fn is_outside(input: VertexOutput) -> bool {
    return outside_region(input.position) || outside_clip_point(input.position);
}
