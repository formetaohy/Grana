struct Uniforms {
    uv_center: vec4<f32>,
    uv_scale: vec4<f32>,
    plane: vec4<f32>,
    axes: vec4<u32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    dims: vec4<f32>,
    box_min: vec4<f32>,
    world: vec4<f32>,
    mapping: vec4<u32>,
    channel_opacity: array<vec4<f32>, 2>,
    channel_window: array<vec4<f32>, 2>,
    channel_inverse: array<vec4<f32>, 2>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var lut: texture_2d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(0) @binding(3) var<storage, read> bricks: array<BrickRecord>;
@group(1) @binding(0) var atlases: binding_array<texture_3d<f32>, 8>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn channel_opacity(index: u32) -> f32 {
    return uniforms.channel_opacity[index / 4u][index % 4u];
}

fn channel_sample(index: u32, unit: f32) -> vec4<f32> {
    let start = uniforms.channel_window[index / 4u][index % 4u];
    let inverse = uniforms.channel_inverse[index / 4u][index % 4u];
    let position = windowed_unit(start, inverse, unit);
    return textureSampleLevel(lut, lut_sampler, vec2<f32>(position, lut_row(index)), 0.0);
}

fn sample_channel(index: u32, uvw: vec3<f32>) -> f32 {
    let cell = brick_cell(uniforms.grid.xyz, uvw);
    let record = bricks[brick_index(uniforms.grid.xyz, cell)];
    if (record.slot == 0u) {
        return 0.0;
    }
    let texel = atlas_texel(
        uniforms.grid.xyz,
        uniforms.atlas.xyz,
        uniforms.dims.xyz,
        record.slot - 1u,
        uvw,
    );
    return textureSampleLevel(atlases[index], atlas_sampler, texel, 0.0).r;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let u = uniforms.uv_center.x + input.ndc.x * uniforms.uv_scale.x;
    let v = uniforms.uv_center.y + input.ndc.y * uniforms.uv_scale.y;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return vec4<f32>(background(input.ndc), 1.0);
    }
    let axis = uniforms.axes.x;
    var uvw = vec3<f32>(0.0);
    uvw[axis] = uniforms.plane.x;
    if (axis == 0u) {
        uvw.y = u;
        uvw.z = v;
    } else if (axis == 1u) {
        uvw.x = u;
        uvw.z = v;
    } else {
        uvw.x = u;
        uvw.y = v;
    }
    var color = vec3<f32>(0.0);
    let count = uniforms.axes.y;
    for (var channel = 0u; channel < count; channel += 1u) {
        let opacity = channel_opacity(channel);
        if (opacity > 0.0) {
            let sample = channel_sample(channel, sample_channel(channel, uvw));
            color += sample.rgb * opacity;
        }
    }
    return vec4<f32>(color + background(input.ndc), 1.0);
}
