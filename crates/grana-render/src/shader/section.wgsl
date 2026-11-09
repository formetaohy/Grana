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

fn locate(uvw: vec3<f32>, point: vec3<f32>) -> SectionHit {
    let cell = brick_cell(section.grid.xyz, section.dims.xyz, uvw);
    let record = bricks[section.bases.x + brick_index(section.grid.xyz, cell)];
    let slot = brick_slot(record);
    if (slot != 0u) {
        return SectionHit(select(0u, slot, record.flags != 0u), uvw, section.grid.xyz, section.dims.xyz);
    }
    if (brick_hollow(record) || section.fallback_grid.w == 0u) {
        return SectionHit(0u, uvw, section.grid.xyz, section.dims.xyz);
    }
    let local = fallback_local(point);
    let other = brick_cell(section.fallback_grid.xyz, section.fallback_dims.xyz, local);
    let fallback = brick_slot(bricks[section.bases.y + brick_index(section.fallback_grid.xyz, other)]);
    return SectionHit(fallback, local, section.fallback_grid.xyz, section.fallback_dims.xyz);
}

fn sample_section_pack(pack: u32, uvw: vec3<f32>, point: vec3<f32>) -> vec4<f32> {
    let hit = locate(uvw, point);
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
    let base = section_point(input.ndc + section.jitter.xy);
    let count = section.channels.x;
    let packs = (count + PACK - 1u) / PACK;
    let mode = u32(section.projection.x);
    let samples = u32(section.projection.z);
    let half = section.projection.y;
    var color = vec4<f32>(0.0);
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        var peak = vec4<f32>(0.0);
        var sum = vec4<f32>(0.0);
        var valid = 0u;
        for (var sample = 0u; sample < samples; sample += 1u) {
            var point = base;
            if (samples > 1u) {
                let t = -half + 2.0 * half * ((f32(sample) + 0.5) / f32(samples));
                point = base + section.plane.xyz * t;
            }
            if (outside_region(point) || outside_clip(point)) {
                continue;
            }
            let uvw = section_local(point);
            if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) {
                continue;
            }
            let values = sample_section_pack(pack, uvw, point);
            peak = max(peak, values);
            sum += values;
            valid += 1u;
        }
        var values = peak;
        if (mode == 2u) {
            values = select(vec4<f32>(0.0), sum / f32(max(valid, 1u)), valid > 0u);
        }
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0) {
                continue;
            }
            let sample = channel_coverage(channel, values[component]);
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
