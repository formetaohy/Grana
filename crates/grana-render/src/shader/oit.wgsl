struct OitFragment {
    next: u32,
    depth: f32,
    color: vec2<u32>,
}

@group(4) @binding(0) var<storage, read_write> oit_nodes: array<OitFragment>;
@group(4) @binding(1) var<storage, read_write> oit_heads: array<atomic<u32>>;
@group(4) @binding(2) var<storage, read_write> oit_counter: atomic<u32>;

fn oit_push(origin: vec2<i32>, size: vec2<i32>, pixel: vec2<i32>, depth: f32, color: vec4<f32>) {
    let local = pixel - origin;
    if (local.x < 0 || local.y < 0 || local.x >= size.x || local.y >= size.y) {
        return;
    }
    let slot = atomicAdd(&oit_counter, 1u);
    if (slot >= arrayLength(&oit_nodes)) {
        return;
    }
    var fragment: OitFragment;
    fragment.next = atomicExchange(&oit_heads[local.y * size.x + local.x], slot + 1u);
    fragment.depth = depth;
    fragment.color = vec2<u32>(pack2x16float(color.rg), pack2x16float(color.ba));
    oit_nodes[slot] = fragment;
}
