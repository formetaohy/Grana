struct IndexedArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

struct Counters {
    counts: array<atomic<u32>, 4>,
    cursors: array<atomic<u32>, 4>,
}

const COUNTER_OPAQUE: u32 = 0u;
const COUNTER_LAYER: u32 = 1u;
const COUNTER_SHADOW: u32 = 2u;

const OPAQUE: u32 = 0u;
const LAYER: u32 = 1u;
const PICK: u32 = 2u;
const SHADOW: u32 = 3u;

@group(0) @binding(0) var<storage, read_write> counters: Counters;
@group(0) @binding(1) var<storage, read_write> args: array<IndexedArgs>;

@compute @workgroup_size(1)
fn mesh_args(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x > 0u) {
        return;
    }
    let opaque = atomicLoad(&counters.counts[COUNTER_OPAQUE]);
    let layer = atomicLoad(&counters.counts[COUNTER_LAYER]);
    let shadow = atomicLoad(&counters.counts[COUNTER_SHADOW]);
    args[OPAQUE] = IndexedArgs(opaque, 1u, 0u, 0, 0u);
    args[LAYER] = IndexedArgs(layer, 1u, opaque, 0, 0u);
    args[PICK] = IndexedArgs(opaque + layer, 1u, 0u, 0, 0u);
    args[SHADOW] = IndexedArgs(opaque + layer + shadow, 1u, 0u, 0, 0u);
}
