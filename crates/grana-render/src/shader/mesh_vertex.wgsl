fn object_position(input: VertexInput) -> vec3<f32> {
    let object = objects[input.object];
    return object.minimum.xyz + input.position.xyz * object.extent.xyz;
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let position = object_position(input);
    let clip = mesh.view_projection * vec4<f32>(position, 1.0);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(
        clip.xy + mesh.jitter.xy * clip.w + lens_shift(clip.w),
        clip.zw,
    );
    output.normal = octahedral_decode(input.normal * 2.0 - vec2<f32>(1.0));
    output.position = position;
    output.object = input.object;
    return output;
}
