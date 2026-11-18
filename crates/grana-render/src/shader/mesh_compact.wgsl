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

struct Counters {
    counts: array<atomic<u32>, 4>,
    cursors: array<atomic<u32>, 4>,
}

const OPAQUE: u32 = 0u;
const LAYER: u32 = 1u;
const SHADOW: u32 = 2u;
const HIDDEN: u32 = 3u;

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(2) @binding(0) var<storage, read> styles: array<StyleData>;
@group(3) @binding(0) var<storage, read> indices: array<u32>;
@group(3) @binding(1) var<storage, read_write> pool: array<u32>;
@group(3) @binding(2) var<storage, read_write> counters: Counters;

fn outside_sphere(center: vec3<f32>, radius: f32) -> bool {
    for (var index = 0u; index < 6u; index += 1u) {
        let plane = frame.planes[index];
        if (dot(plane.xyz, center) + plane.w < -radius) {
            return true;
        }
    }
    return false;
}

fn slot(visible: bool, inside: bool, opaque: bool) -> u32 {
    if (!visible) {
        return HIDDEN;
    }
    if (!inside) {
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
    return slot(style.flags.x >= 0.5, inside, style.flags.z >= 0.5);
}

@compute @workgroup_size(64)
fn mesh_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let kind = classify(index);
    let count = objects[index].index_count;
    if (kind >= HIDDEN || count == 0u) {
        return;
    }
    atomicAdd(&counters.counts[kind], count);
}

@compute @workgroup_size(64)
fn mesh_compact(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= arrayLength(&objects)) {
        return;
    }
    let kind = classify(index);
    let count = objects[index].index_count;
    if (kind >= HIDDEN || count == 0u) {
        return;
    }
    var offset = 0u;
    if (kind > OPAQUE) {
        offset = atomicLoad(&counters.counts[OPAQUE]);
        if (kind > LAYER) {
            offset += atomicLoad(&counters.counts[LAYER]);
        }
    }
    let base = offset + atomicAdd(&counters.cursors[kind], count);
    let first = objects[index].first_index;
    for (var at = 0u; at < count; at += 1u) {
        pool[base + at] = indices[first + at];
    }
}
