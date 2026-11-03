struct MarkerInstance {
    center_radius: vec4<f32>,
    color_alpha: vec4<f32>,
}

struct InstanceArgs {
    vertex_count: u32,
    instance_count: atomic<u32>,
    first_vertex: u32,
    first_instance: u32,
}

@group(1) @binding(0) var<storage, read> markers: array<MarkerInstance>;
@group(1) @binding(1) var<storage, read_write> opaque_order: array<u32>;
@group(1) @binding(2) var<storage, read_write> layer_order: array<u32>;
@group(1) @binding(3) var<storage, read_write> behind_order: array<u32>;
@group(1) @binding(4) var<storage, read_write> front_order: array<u32>;
@group(1) @binding(5) var<storage, read_write> opaque_args: InstanceArgs;
@group(1) @binding(6) var<storage, read_write> layer_args: InstanceArgs;
@group(1) @binding(7) var<storage, read_write> behind_args: InstanceArgs;
@group(1) @binding(8) var<storage, read_write> front_args: InstanceArgs;

fn outside_sphere(center: vec3<f32>, radius: f32) -> bool {
    for (var index = 0u; index < 6u; index += 1u) {
        let plane = frame.planes[index];
        if (dot(plane.xyz, center) + plane.w < -radius) {
            return true;
        }
    }
    return false;
}

fn outside_section(center: vec3<f32>, radius: f32) -> bool {
    let world = section.world.xyz;
    let uvw = (center - section.box_min.xyz) / world;
    let axis = section.mapping.z;
    if (abs(uvw[axis] - section.plane.x) * world[axis] > radius) {
        return true;
    }
    let u = section.mapping.x;
    let v = section.mapping.y;
    if (abs(uvw[u] - section.uv_center.x) - radius / world[u] > section.uv_scale.x) {
        return true;
    }
    if (abs(uvw[v] - section.uv_center.y) - radius / world[v] > section.uv_scale.y) {
        return true;
    }
    return false;
}

fn append_volume(index: u32, opaque: bool) {
    if (opaque) {
        let slot = atomicAdd(&opaque_args.instance_count, 1u);
        opaque_order[slot] = index;
    } else {
        let slot = atomicAdd(&layer_args.instance_count, 1u);
        layer_order[slot] = index;
    }
}

fn append_section(index: u32, front: bool) {
    if (front) {
        let slot = atomicAdd(&front_args.instance_count, 1u);
        front_order[slot] = index;
    } else {
        let slot = atomicAdd(&behind_args.instance_count, 1u);
        behind_order[slot] = index;
    }
}

@compute @workgroup_size(64)
fn marker_cull_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&markers)) {
        return;
    }
    let data = markers[index];
    if (!outside_sphere(data.center_radius.xyz, data.center_radius.w)
        && !outside_region_sphere(data.center_radius.xyz, data.center_radius.w)) {
        append_volume(index, data.color_alpha.a >= 1.0);
    }
}

fn cull_section_side(index: u32, front: bool) {
    if (index >= arrayLength(&markers)) {
        return;
    }
    let data = markers[index];
    if (outside_section(data.center_radius.xyz, data.center_radius.w)
        || outside_region_sphere(data.center_radius.xyz, data.center_radius.w)) {
        return;
    }
    let uvw = (data.center_radius.xyz - section.box_min.xyz) / section.world.xyz;
    let ahead = uvw[section.mapping.z] >= section.plane.x;
    if (ahead == front) {
        append_section(index, front);
    }
}

@compute @workgroup_size(64)
fn marker_cull_section_behind(@builtin(global_invocation_id) id: vec3<u32>) {
    cull_section_side(id.x, false);
}

@compute @workgroup_size(64)
fn marker_cull_section_front(@builtin(global_invocation_id) id: vec3<u32>) {
    cull_section_side(id.x, true);
}
