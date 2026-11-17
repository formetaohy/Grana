@vertex
fn vs_shadow(input: VertexInput) -> ShadowOutput {
    let coordinate = shadow_coordinate(mesh.light_view, object_position(input));
    var output: ShadowOutput;
    output.clip_position = vec4<f32>(
        coordinate.x * 2.0 - 1.0,
        1.0 - coordinate.y * 2.0,
        coordinate.z,
        1.0,
    );
    return output;
}
