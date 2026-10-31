@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 16>;
@group(2) @binding(1) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn sample_section(channel: u32, uvw: vec3<f32>) -> f32 {
    let cell = brick_cell(section.grid.xyz, uvw);
    let record = bricks[brick_index(section.grid.xyz, cell)];
    if (record.slot == 0u || record.flags == 0u) {
        return 0.0;
    }
    let texel = atlas_texel(
        section.grid.xyz,
        section.atlas.xyz,
        section.dims.xyz,
        record.slot - 1u,
        uvw,
    );
    return textureSampleLevel(atlases[channel], atlas_sampler, texel, 0.0).r;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let u = section.uv_center.x + input.ndc.x * section.uv_scale.x;
    let v = section.uv_center.y + input.ndc.y * section.uv_scale.y;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return vec4<f32>(background(input.ndc), 1.0);
    }
    let axis = section.mapping.z;
    var uvw = vec3<f32>(0.0);
    uvw[axis] = section.plane.x;
    uvw[section.mapping.x] = u;
    uvw[section.mapping.y] = v;
    if (outside_crop(section.box_min.xyz + uvw * section.world.xyz)) {
        return vec4<f32>(background(input.ndc), 1.0);
    }
    var color = vec3<f32>(0.0);
    let count = section.axes.x;
    for (var channel = 0u; channel < count; channel += 1u) {
        let opacity = channel_opacity(channel);
        if (opacity > 0.0) {
            let sample = channel_sample(channel, sample_section(channel, uvw));
            color += sample.rgb * opacity;
        }
    }
    return vec4<f32>(color + background(input.ndc), 1.0);
}
