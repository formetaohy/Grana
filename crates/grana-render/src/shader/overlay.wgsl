struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

@group(0) @binding(0) var overlay_texture: texture_2d<f32>;

@group(0) @binding(1) var overlay_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.position = position;
    out.uv = uv;
    out.tint = tint;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(overlay_texture, overlay_sampler, in.uv);
    let light = max(texel.r, max(texel.g, texel.b)) * in.tint.a;
    return vec4<f32>(in.tint.rgb * light, light);
}
