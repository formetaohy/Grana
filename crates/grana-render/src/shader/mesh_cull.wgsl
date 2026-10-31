struct ObjectData {
    center: vec4<f32>,
    radius: f32,
    first_index: u32,
    index_count: u32,
    reserved: u32,
}

struct StyleData {
    color: vec4<f32>,
    flags: vec4<f32>,
}

struct IndexedArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<StyleData>;
@group(3) @binding(0) var<storage, read_write> commands: array<IndexedArgs>;

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

@compute @workgroup_size(64)
fn mesh_cull_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let object = objects[index];
    var args = IndexedArgs(0u, 0u, 0u, 0, 0u);
    if (styles[index].flags.x >= 0.5
        && object.index_count > 0u
        && !outside_sphere(object.center.xyz, object.radius)
        && !outside_crop_sphere(object.center.xyz, object.radius)) {
        args.index_count = object.index_count;
        args.instance_count = 1u;
        args.first_index = object.first_index;
    }
    commands[index] = args;
}

@compute @workgroup_size(64)
fn mesh_cull_section(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let object = objects[index];
    var args = IndexedArgs(0u, 0u, 0u, 0, 0u);
    if (styles[index].flags.x >= 0.5
        && object.index_count > 0u
        && !outside_section(object.center.xyz, object.radius)
        && !outside_crop_sphere(object.center.xyz, object.radius)) {
        args.index_count = object.index_count;
        args.instance_count = 1u;
        args.first_index = object.first_index;
    }
    commands[index] = args;
}
