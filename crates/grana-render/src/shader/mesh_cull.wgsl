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

const VOLUME_NARROW: u32 = 0u;
const VOLUME_WIDE: u32 = 1u;
const SECTION_NARROW: u32 = 2u;
const SECTION_WIDE: u32 = 3u;
const SHADOW_NARROW: u32 = 4u;
const SHADOW_WIDE: u32 = 5u;

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

fn command(first_index: u32, index_count: u32, base_vertex: u32) -> IndexedArgs {
    return IndexedArgs(index_count, 1u, first_index, i32(base_vertex), 0u);
}

fn narrow_command(object: ObjectData, base_vertex: u32) -> IndexedArgs {
    if (object.narrow_count == 0u) {
        return IndexedArgs(0u, 0u, 0u, 0, 0u);
    }
    return command(object.narrow_first, object.narrow_count, base_vertex);
}

fn wide_command(object: ObjectData, base_vertex: u32) -> IndexedArgs {
    if (object.wide_count == 0u) {
        return IndexedArgs(0u, 0u, 0u, 0, 0u);
    }
    return command(object.wide_first, object.wide_count, base_vertex);
}

@compute @workgroup_size(64)
fn mesh_cull_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let count = arrayLength(&objects);
    let object = objects[index];
    var narrow = IndexedArgs(0u, 0u, 0u, 0, 0u);
    var wide = IndexedArgs(0u, 0u, 0u, 0, 0u);
    if (styles[index].flags.x >= 0.5
        && !outside_sphere(object.center_radius.xyz, object.center_radius.w)
        && !outside_region_sphere(object.center_radius.xyz, object.center_radius.w)) {
        narrow = narrow_command(object, object.vertex_base);
        wide = wide_command(object, object.vertex_base);
    }
    commands[index + count * VOLUME_NARROW] = narrow;
    commands[index + count * VOLUME_WIDE] = wide;
}

@compute @workgroup_size(64)
fn mesh_cull_section(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let count = arrayLength(&objects);
    let object = objects[index];
    var narrow = IndexedArgs(0u, 0u, 0u, 0, 0u);
    var wide = IndexedArgs(0u, 0u, 0u, 0, 0u);
    if (styles[index].flags.x >= 0.5
        && !outside_section(object.center_radius.xyz, object.center_radius.w)
        && !outside_region_sphere(object.center_radius.xyz, object.center_radius.w)) {
        narrow = narrow_command(object, object.vertex_base);
        wide = wide_command(object, object.vertex_base);
    }
    commands[index + count * SECTION_NARROW] = narrow;
    commands[index + count * SECTION_WIDE] = wide;
}

@compute @workgroup_size(64)
fn mesh_cull_shadow(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let count = arrayLength(&objects);
    let object = objects[index];
    var narrow = IndexedArgs(0u, 0u, 0u, 0, 0u);
    var wide = IndexedArgs(0u, 0u, 0u, 0, 0u);
    if (styles[index].flags.x >= 0.5) {
        narrow = narrow_command(object, object.vertex_base);
        wide = wide_command(object, object.vertex_base);
    }
    commands[index + count * SHADOW_NARROW] = narrow;
    commands[index + count * SHADOW_WIDE] = wide;
}
