@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 4>;
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

struct SectionHit {
    slot: u32,
    uvw: vec3<f32>,
    grid: vec3<u32>,
    dims: vec3<f32>,
}

fn locate(uvw: vec3<f32>) -> SectionHit {
    let cell = brick_cell(section.grid.xyz, section.dims.xyz, uvw);
    let record = bricks[section.bases.x + brick_index(section.grid.xyz, cell)];
    let slot = brick_slot(record);
    if (slot != 0u) {
        return SectionHit(select(0u, slot, record.flags != 0u), uvw, section.grid.xyz, section.dims.xyz);
    }
    if (brick_hollow(record) || section.fallback_grid.w == 0u) {
        return SectionHit(0u, uvw, section.grid.xyz, section.dims.xyz);
    }
    let world = section.box_min.xyz + uvw * section.world.xyz;
    let local = (world - section.fallback_min.xyz) / section.fallback_world.xyz;
    let other = brick_cell(section.fallback_grid.xyz, section.fallback_dims.xyz, local);
    let fallback = brick_slot(bricks[section.bases.y + brick_index(section.fallback_grid.xyz, other)]);
    return SectionHit(fallback, local, section.fallback_grid.xyz, section.fallback_dims.xyz);
}

fn sample_section_pack(pack: u32, uvw: vec3<f32>) -> vec4<f32> {
    let hit = locate(uvw);
    if (hit.slot == 0u) {
        return vec4<f32>(0.0);
    }
    let texel = atlas_texel(
        hit.grid,
        section.atlas.xyz,
        hit.dims,
        hit.slot - 1u,
        hit.uvw,
    );
    return textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let u = section.uv_center.x + input.ndc.x * section.uv_scale.x;
    let v = section.uv_center.y + input.ndc.y * section.uv_scale.y;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return vec4<f32>(0.0);
    }
    let axis = section.mapping.z;
    var uvw = vec3<f32>(0.0);
    uvw[axis] = section.plane.x;
    uvw[section.mapping.x] = u;
    uvw[section.mapping.y] = v;
    if (outside_region(section.box_min.xyz + uvw * section.world.xyz)
        || outside_clip(section.box_min.xyz + uvw * section.world.xyz)) {
        return vec4<f32>(0.0);
    }
    var color = vec4<f32>(0.0);
    let count = section.axes.x;
    let packs = (count + PACK - 1u) / PACK;
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        let values = sample_section_pack(pack, uvw);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0) {
                continue;
            }
            let sample = channel_sample(channel, values[component]);
            let alpha = sample.a * opacity;
            if (alpha > 0.0) {
                color = vec4<f32>(
                    sample.rgb * alpha + color.rgb * (1.0 - alpha),
                    alpha + color.a * (1.0 - alpha),
                );
            }
        }
    }
    return color;
}
