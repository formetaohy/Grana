struct ResolveOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> ResolveOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: ResolveOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

@fragment
fn fs_main(input: ResolveOutput) -> @location(0) vec4<f32> {
    let layers = append_tail(gather_layers(input.clip_position), input.clip_position);
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    for (var index = 0u; index < layers.count; index += 1u) {
        let layer = layer_color(layers.color[index]);
        radiance += transmittance * layer.rgb;
        transmittance *= 1.0 - layer.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
