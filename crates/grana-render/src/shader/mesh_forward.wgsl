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

@fragment
fn fs_oit(input: VertexOutput) -> Residual {
    var output = oit_residual(0.0, vec3<f32>(0.0), 0.0);
    if (is_outside(input)) {
        return output;
    }
    let origin = vec2<i32>(mesh.viewport.xy);
    let size = vec2<i32>(mesh.viewport.zw);
    let pixel = vec2<i32>(input.clip_position.xy);
    let index = oit_index(origin, size, pixel);
    if (index < 0) {
        return output;
    }
    let sample = shade(
        styles[input.object],
        input.normal,
        input.position,
        ambient_occlusion(input.clip_position.xy),
    );
    let premultiplied = vec4<f32>(sample.rgb * sample.a, sample.a);
    let depth = input.clip_position.z;
    if (oit_insert(u32(index), mesh.oit.y, mesh.oit.z, depth, premultiplied)) {
        return output;
    }
    return oit_residual(sample.a, sample.rgb, depth);
}

@fragment
fn fs_pick(input: VertexOutput) -> @location(0) vec2<u32> {
    if (is_outside(input)) {
        discard;
    }
    return vec2<u32>(1u, pick_ids[input.object]);
}
