@group(3) @binding(0) var light_volume: texture_storage_3d<rgba16float, write>;

const LIGHT_STEPS: u32 = 64u;
const AMBIENT_REACH: f32 = 1.5;
const MINIMUM_ALPHA: f32 = 1.0e-4;

@compute @workgroup_size(4, 4, 4)
fn light_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(light_volume);
    if (any(id >= dims)) {
        return;
    }
    let uvw = (vec3<f32>(id) + vec3<f32>(0.5)) / vec3<f32>(dims);
    let voxel_world = frame.box_size.xyz / frame.dims.xyz;
    let direction = frame.light.xyz;
    let voxels_per_world = length(direction / voxel_world);
    let step = length(frame.box_size.xyz) / f32(LIGHT_STEPS);
    var position = frame.box_min.xyz + uvw * frame.box_size.xyz;
    var optical_depth = 0.0;
    for (var index = 0u; index < LIGHT_STEPS; index += 1u) {
        position += direction * step;
        let local = (position - frame.box_min.xyz) / frame.box_size.xyz;
        if (any(local < vec3<f32>(0.0)) || any(local > vec3<f32>(1.0))) {
            break;
        }
        let per_voxel = channel_alpha(local);
        if (per_voxel > MINIMUM_ALPHA) {
            optical_depth += -log(1.0 - per_voxel) * step * voxels_per_world;
        }
    }
    let reach = AMBIENT_REACH / vec3<f32>(dims);
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
        occlusion += channel_alpha(at);
    }
    occlusion /= 6.0;
    textureStore(light_volume, id, vec4<f32>(exp(-optical_depth), occlusion, 0.0, 1.0));
}
