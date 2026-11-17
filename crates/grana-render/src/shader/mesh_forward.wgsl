@group(5) @binding(0) var<storage, read> pick_ids: array<u32>;

@fragment
fn fs_opaque(input: VertexOutput) -> @location(0) vec4<f32> {
    if (is_outside(input)) {
        discard;
    }
    return shade(
        styles[input.object],
        input.normal,
        input.position,
        ambient_occlusion(input.clip_position.xy),
    );
}

@fragment
fn fs_oit(input: VertexOutput) -> Residual {
    var output = oit_residual(mesh.oit.z, 0.0, vec3<f32>(0.0), 0.0);
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
    return oit_residual(mesh.oit.z, sample.a, sample.rgb, depth);
}

@fragment
fn fs_pick(input: VertexOutput) -> @location(0) vec2<u32> {
    if (is_outside(input)) {
        discard;
    }
    return vec2<u32>(1u, pick_ids[input.object]);
}
