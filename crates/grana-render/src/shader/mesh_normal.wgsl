struct NormalOutput {
    @location(0) normal: vec2<f32>,
}

@fragment
fn fs_normal(input: VertexOutput) -> NormalOutput {
    if (is_outside(input)) {
        discard;
    }
    return NormalOutput(octahedral_encode(normalize(input.normal)));
}
