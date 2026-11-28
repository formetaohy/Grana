struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    first_index: u32,
    index_count: u32,
    reserved: vec2<u32>,
}

struct StyleData {
    color: vec4<f32>,
    flags: vec4<f32>,
    surface: vec4<f32>,
}

struct IndexedArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

struct Counters {
    counts: array<atomic<u32>, 4>,
}

const REGIONS: u32 = 4u;
const OPAQUE: u32 = 0u;
const LAYER: u32 = 1u;
const PICK: u32 = 2u;
const SHADOW: u32 = 3u;

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<StyleData>;
@group(3) @binding(0) var<storage, read_write> args: array<IndexedArgs>;
@group(3) @binding(1) var<storage, read_write> counters: Counters;

fn outside_sphere(center: vec3<f32>, radius: f32) -> bool {
    for (var index = 0u; index < 6u; index += 1u) {
        let plane = frame.planes[index];
        if (dot(plane.xyz, center) + plane.w < -radius) {
            return true;
        }
    }
    return false;
}

fn inside_view(center: vec3<f32>, radius: f32) -> bool {
    return !outside_sphere(center, radius) && !outside_region_sphere(center, radius);
}

fn publish(index: u32, kind: u32) {
    let capacity = arrayLength(&args) / REGIONS;
    let slot = atomicAdd(&counters.counts[kind], 1u);
    let object = objects[index];
    args[kind * capacity + slot] = IndexedArgs(
        object.index_count,
        1u,
        object.first_index,
        0,
        0u,
    );
}

@compute @workgroup_size(64)
fn mesh_cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let object = objects[index];
    if (object.index_count == 0u) {
        return;
    }
    let style = styles[index];
    if (style.flags.x < 0.5) {
        return;
    }
    if (frame.march.y > 0.0) {
        publish(index, SHADOW);
    }
    if (!inside_view(object.center_radius.xyz, object.center_radius.w)) {
        return;
    }
    publish(index, select(LAYER, OPAQUE, style.flags.z >= 0.5));
    publish(index, PICK);
}
