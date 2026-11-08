@group(3) @binding(0) var occlusion_output: texture_storage_3d<rgba8unorm, write>;

const AMBIENT_REACH: f32 = 0.02;

@compute @workgroup_size(4, 4, 4)
fn occlusion_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(occlusion_output);
    if (any(id >= dims)) {
        return;
    }
    let uvw = (vec3<f32>(id) + vec3<f32>(0.5)) / vec3<f32>(dims);
    let reach = AMBIENT_REACH * length(frame.box_size.xyz) / frame.box_size.xyz;
    let offsets = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(0.0, 0.0, -1.0),
    );
    var occlusion = 0.0;
    for (var index = 0u; index < 6u; index += 1u) {
        let at = clamp(uvw + offsets[index] * reach, vec3<f32>(0.0), vec3<f32>(1.0));
        occlusion += channel_alpha(0u, at);
    }
    textureStore(occlusion_output, id, vec4<f32>(occlusion / 6.0, 0.0, 0.0, 1.0));
}
