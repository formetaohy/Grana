@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 4>;
@group(2) @binding(1) var atlas_sampler: sampler;

const ISO_EXTINCTION: f32 = 8.0;
const ISO_SOFTNESS: f32 = 0.04;
const CELL_SIDE: f32 = 8.0;
const CELL_AXIS: f32 = 4.0;

struct LevelFrame {
    dims: vec3<f32>,
    grid: vec3<u32>,
    origin: vec3<f32>,
}

fn level_frame(band: u32) -> LevelFrame {
    return LevelFrame(
        frame.dims[band].xyz,
        frame.grids[band].xyz,
        frame.origins[band].xyz,
    );
}

fn level_record(band: u32, index: u32) -> BrickRecord {
    return bricks[frame.bases[band] + index];
}

fn band_uvw(band: u32, uvw: vec3<f32>) -> vec3<f32> {
    if (band == 0u) {
        return uvw;
    }
    return level_local(band, level_world(0u, uvw));
}

fn cell_voxel(band: u32, uvw: vec3<f32>, brick: vec3<u32>) -> vec3<f32> {
    let g = level_frame(band);
    return clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims - brick_origin(brick);
}

fn cell_coord(local: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(
        clamp(floor(local / CELL_SIDE), vec3<f32>(0.0), vec3<f32>(CELL_AXIS - 1.0)),
    );
}

fn cell_index(cell: vec3<u32>) -> u32 {
    return cell.x + u32(CELL_AXIS) * (cell.y + u32(CELL_AXIS) * cell.z);
}

fn cell_edge(band: u32, uvw: vec3<f32>, brick: vec3<u32>, direction: vec3<f32>) -> vec3<f32> {
    let g = level_frame(band);
    let origin = brick_origin(brick);
    let base = floor(cell_voxel(band, uvw, brick) / CELL_SIDE) * CELL_SIDE;
    var edge = vec3<f32>(0.0);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let limit = clamp(g.dims[axis] - origin[axis] - base[axis], 0.0, CELL_SIDE);
        let far = select(base[axis] + limit, base[axis], direction[axis] < 0.0);
        edge[axis] = (origin[axis] + far) / g.dims[axis];
    }
    return edge;
}

fn cell_delta(
    band: u32,
    uvw: vec3<f32>,
    brick: vec3<u32>,
    direction: vec3<f32>,
    minimum: f32,
) -> f32 {
    let edge = cell_edge(band, uvw, brick, direction);
    var delta = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        if (abs(step) > 1.0e-9) {
            delta = min(delta, (edge[axis] - uvw[axis]) / step);
        }
    }
    return max(delta, minimum);
}

fn sample_pack(band: u32, pack: u32, uvw: vec3<f32>) -> vec4<f32> {
    let g = level_frame(band);
    let cell = brick_cell(g.grid, g.dims, uvw);
    let record = level_record(band, brick_index(g.grid, cell));
    let slot = brick_display_slot(record);
    if (slot == 0u) {
        return vec4<f32>(0.0);
    }
    let texel = atlas_texel(g.grid, frame.atlas.xyz, g.dims, slot - 1u, uvw);
    return textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
}

fn level_voxels_per_world(band: u32, direction: vec3<f32>) -> f32 {
    let g = level_frame(band);
    return length(level_rate(band, direction) * g.dims);
}

fn channel_extinction(band: u32, uvw: vec3<f32>) -> f32 {
    let position = band_uvw(band, uvw);
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var extinction = 0.0;
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        let values = sample_pack(band, pack, position);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0) {
                continue;
            }
            if (channel_isosurface(channel)) {
                let inverse = channels[channel].inverse;
                if (inverse > 0.0) {
                    let softness = ISO_SOFTNESS / inverse;
                    let threshold = channel_threshold(channel);
                    let ramp = smoothstep(
                        threshold - softness,
                        threshold + softness,
                        values[component],
                    );
                    extinction += ISO_EXTINCTION * ramp * opacity;
                }
                continue;
            }
            if (channel_maximum(channel)) {
                continue;
            }
            let alpha = clamp(channel_sample(channel, values[component]).a, 0.0, 0.998);
            extinction += -log(1.0 - alpha) * opacity;
        }
    }
    return extinction;
}

fn ray_uvw(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> vec3<f32> {
    return level_local(band, origin + direction * t);
}

fn boundary_delta(band: u32, uvw: vec3<f32>, direction: vec3<f32>, minimum: f32) -> f32 {
    let g = level_frame(band);
    let edge = brick_edge(uvw, g.dims, direction);
    var delta = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        if (abs(step) > 1.0e-9) {
            delta = min(delta, (edge[axis] - uvw[axis]) / step);
        }
    }
    return max(delta, minimum);
}

