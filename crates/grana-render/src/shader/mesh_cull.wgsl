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

const OPAQUE_NARROW: u32 = 0u;
const OPAQUE_WIDE: u32 = 1u;
const LAYER_NARROW: u32 = 2u;
const LAYER_WIDE: u32 = 3u;
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

fn empty() -> IndexedArgs {
    return IndexedArgs(0u, 0u, 0u, 0, 0u);
}

fn command(first_index: u32, index_count: u32, base_vertex: u32) -> IndexedArgs {
    if (index_count == 0u) {
        return empty();
    }
    return IndexedArgs(index_count, 1u, first_index, i32(base_vertex), 0u);
}

@compute @workgroup_size(64)
fn mesh_cull_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let count = arrayLength(&objects);
    let object = objects[index];
    let style = styles[index];
    var opaque_narrow = empty();
    var opaque_wide = empty();
    var layer_narrow = empty();
    var layer_wide = empty();
    if (style.flags.x >= 0.5
        && !outside_sphere(object.center_radius.xyz, object.center_radius.w)
        && !outside_region_sphere(object.center_radius.xyz, object.center_radius.w)) {
        let narrow = command(object.narrow_first, object.narrow_count, object.vertex_base);
        let wide = command(object.wide_first, object.wide_count, object.vertex_base);
        if (style.flags.z >= 0.5) {
            opaque_narrow = narrow;
            opaque_wide = wide;
        } else {
            layer_narrow = narrow;
            layer_wide = wide;
        }
    }
    commands[index + count * OPAQUE_NARROW] = opaque_narrow;
    commands[index + count * OPAQUE_WIDE] = opaque_wide;
    commands[index + count * LAYER_NARROW] = layer_narrow;
    commands[index + count * LAYER_WIDE] = layer_wide;
}

@compute @workgroup_size(64)
fn mesh_cull_shadow(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let count = arrayLength(&objects);
    let object = objects[index];
    var narrow = empty();
    var wide = empty();
    if (styles[index].flags.x >= 0.5) {
        narrow = command(object.narrow_first, object.narrow_count, object.vertex_base);
        wide = command(object.wide_first, object.wide_count, object.vertex_base);
    }
    commands[index + count * SHADOW_NARROW] = narrow;
    commands[index + count * SHADOW_WIDE] = wide;
}
