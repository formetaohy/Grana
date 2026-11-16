struct VertexOut {
    @builtin(position) position: vec4<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VertexOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VertexOut;
    out.position = vec4<f32>(corners[index], 0.0, 1.0);
    return out;
}

@fragment
fn fs_clear(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}

@fragment
fn fs_copy(in: VertexOut) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(in.position.xy), 0);
}
