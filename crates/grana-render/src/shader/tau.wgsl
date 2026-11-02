@group(3) @binding(0) var tau_output: texture_storage_3d<rgba8unorm, write>;

@compute @workgroup_size(4, 4, 1)
fn tau_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(tau_output);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + vec2<f32>(0.5)) / vec2<f32>(dims.xy);
    let step = 1.0 / f32(dims.z);
    let voxels_per_world = length(frame.light.xyz * frame.dims.xyz / frame.box_size.xyz);
    let slice = frame.light_reach.x * step;
    var tau = 0.0;
    for (var layer = 0u; layer < dims.z; layer += 1u) {
        let w = (f32(layer) + 0.5) * step;
        let world = (frame.light_inverse * vec4<f32>(uv, w, 1.0)).xyz;
        let uvw = (world - frame.box_min.xyz) / frame.box_size.xyz;
        if (all(uvw >= vec3<f32>(-1.0e-5)) && all(uvw <= vec3<f32>(1.0 + 1.0e-5))) {
            tau += channel_extinction(uvw) * voxels_per_world * slice;
        }
        textureStore(tau_output, vec3<u32>(id.xy, layer), vec4<f32>(encoded_tau(tau), 0.0, 0.0, 1.0));
    }
}
