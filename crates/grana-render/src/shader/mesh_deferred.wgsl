struct DeferredOutput {
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn fs_deferred(@builtin(position) position: vec4<f32>) -> DeferredOutput {
    let pixel = vec2<i32>(position.xy);
    let depth = textureLoad(gbuffer_depth, pixel, 0);
    if (depth >= 1.0) {
        return DeferredOutput(vec4<f32>(0.0));
    }
    let row = textureLoad(gbuffer_id, pixel, 0).r;
    if (row == 0u) {
        return DeferredOutput(vec4<f32>(0.0));
    }
    let stored = textureLoad(gbuffer_normal, pixel, 0);
    let extent = vec2<f32>(textureDimensions(gbuffer_depth));
    let ndc = vec2<f32>(
        (f32(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
        1.0 - (f32(pixel.y) + 0.5) / extent.y * 2.0,
    );
    let clip = mesh.inverse_view_projection * vec4<f32>(ndc, depth, 1.0);
    let world = clip.xyz / clip.w;
    return DeferredOutput(shade(
        styles[row - 1u],
        stored.xyz,
        world,
        ambient_occlusion(position.xy),
    ));
}
