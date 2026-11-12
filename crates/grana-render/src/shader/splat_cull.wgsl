struct SplatInstance {
    center: vec4<f32>,
    sigma_alpha: vec4<f32>,
    color: vec4<f32>,
}

struct InstanceArgs {
    vertex_count: u32,
    instance_count: atomic<u32>,
    first_vertex: u32,
    first_instance: u32,
}

@group(1) @binding(0) var<storage, read> splats: array<SplatInstance>;
@group(1) @binding(1) var<storage, read_write> order: array<u32>;
@group(1) @binding(5) var<storage, read_write> args: InstanceArgs;

const SPLAT_REACH: f32 = 3.0;

fn outside_sphere(center: vec3<f32>, radius: f32) -> bool {
    for (var index = 0u; index < 6u; index += 1u) {
        let plane = frame.planes[index];
        if (dot(plane.xyz, center) + plane.w < -radius) {
            return true;
        }
    }
    return false;
}

@compute @workgroup_size(64)
fn splat_cull_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= frame.tag.z || index >= arrayLength(&splats)) {
        return;
    }
    let data = splats[index];
    let sigma = max(data.sigma_alpha.x, max(data.sigma_alpha.y, data.sigma_alpha.z));
    let reach = SPLAT_REACH * sigma;
    if (!outside_sphere(data.center.xyz, reach) && !outside_region_sphere(data.center.xyz, reach)) {
        let slot = atomicAdd(&args.instance_count, 1u);
        order[slot] = index;
    }
}
