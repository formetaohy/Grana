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
const HIDDEN: u32 = 4u;

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

fn region(visible: bool, inside: bool, opaque: bool, shadows: bool) -> u32 {
    if (!visible) {
        return HIDDEN;
    }
    if (!inside) {
        if (!shadows) {
            return HIDDEN;
        }
        return SHADOW;
    }
    if (opaque) {
        return OPAQUE;
    }
    return LAYER;
}

fn classify(index: u32) -> u32 {
    let object = objects[index];
    let style = styles[index];
    let inside = !outside_sphere(object.center_radius.xyz, object.center_radius.w)
        && !outside_region_sphere(object.center_radius.xyz, object.center_radius.w);
    return region(
        style.flags.x >= 0.5,
        inside,
        style.flags.z >= 0.5,
        frame.march.y > 0.0,
    );
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
    if (objects[index].index_count == 0u) {
        return;
    }
    let kind = classify(index);
    if (kind == HIDDEN) {
        return;
    }
    publish(index, kind);
    if (kind == OPAQUE || kind == LAYER) {
        publish(index, PICK);
    }
}
