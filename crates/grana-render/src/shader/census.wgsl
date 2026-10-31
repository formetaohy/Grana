struct CensusData {
    grid: vec4<u32>,
    dims: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    index: vec4<u32>,
}

@group(1) @binding(0) var<uniform> census: CensusData;
@group(1) @binding(1) var<storage, read_write> counts: array<atomic<u32>>;

@compute @workgroup_size(64)
fn census_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= census.grid.w) {
        return;
    }
    let grid = census.grid.xyz;
    let cell = vec3<u32>(
        index % grid.x,
        (index / grid.x) % grid.y,
        index / (grid.x * grid.y),
    );
    let origin = vec3<f32>(cell) * 32.0;
    let size = min(vec3<f32>(32.0), census.dims.xyz - origin);
    let voxel = census.box_size.xyz / census.dims.xyz;
    let low = census.box_min.xyz + origin * voxel;
    let high = low + size * voxel;
    if (outside_frustum(low, high) || outside_crop_bounds(low, high)) {
        return;
    }
    atomicAdd(&counts[census.index.x], 1u);
}
