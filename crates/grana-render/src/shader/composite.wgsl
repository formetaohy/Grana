@group(5) @binding(0) var<storage, read> occupied: array<u32>;
@group(2) @binding(2) var baked_volume: texture_3d<f32>;

const OCCUPANCY_WORDS: u32 = 65u;
const SAMPLE_LIMIT: u32 = 1024u;
const CELL_LIMIT: u32 = 1024u;
const SEGMENT_ERROR: f32 = 0.02;
const SEGMENT_STRIDE: f32 = 8.0;
const MINIMUM_TAU: f32 = 1.0e-4;

struct March {
    radiance: vec3<f32>,
    transmittance: f32,
    moment: f32,
    mass: f32,
    hit: vec4<f32>,
    normal: vec4<f32>,
}

struct Sight {
    usable: bool,
    missing: bool,
    band: u32,
    cell: vec3<u32>,
    uvw: vec3<f32>,
}

fn occupied_brick(slot: u32) -> f32 {
    return bitcast<f32>(occupied[slot * OCCUPANCY_WORDS]);
}

fn occupied_cell(slot: u32, cell: u32) -> f32 {
    return bitcast<f32>(occupied[slot * OCCUPANCY_WORDS + 1u + cell]);
}

fn baked_sample(texel: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(baked_volume, atlas_sampler, texel, 0.0);
}

fn baked_sight(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> Sight {
    let g = level_frame(band);
    let uvw = ray_uvw(origin, band, t, direction);
    let cell = brick_cell(g.grid, g.dims, uvw);
    let record = level_record(band, brick_index(g.grid, cell));
    let slot = brick_slot(record);
    var usable = false;
    if (slot != 0u) {
        usable = occupied_brick(slot - 1u) > MINIMUM_TAU;
    }
    return Sight(usable, slot == 0u && !brick_hollow(record), band, cell, uvw);
}

fn baked_reachable(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> Sight {
    let count = band_count();
    let first = baked_sight(origin, band, t, direction);
    if (first.usable) {
        return first;
    }
    var chosen = first;
    for (var step = 1u; step < count; step += 1u) {
        let coarser = band + step;
        if (coarser < count) {
            let other = baked_sight(origin, coarser, t, direction);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
        if (step <= band) {
            let finer = band - step;
            let other = baked_sight(origin, finer, t, direction);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
    }
    return chosen;
}

fn baked_normal(texel: vec3<f32>, band: u32, extent: vec3<f32>) -> vec3<f32> {
    let x = baked_sample(texel + vec3<f32>(1.0, 0.0, 0.0) / extent).a
        - baked_sample(texel - vec3<f32>(1.0, 0.0, 0.0) / extent).a;
    let y = baked_sample(texel + vec3<f32>(0.0, 1.0, 0.0) / extent).a
        - baked_sample(texel - vec3<f32>(0.0, 1.0, 0.0) / extent).a;
    let z = baked_sample(texel + vec3<f32>(0.0, 0.0, 1.0) / extent).a
        - baked_sample(texel - vec3<f32>(0.0, 0.0, 1.0) / extent).a;
    let gradient = level_gradient(band, vec3<f32>(x, y, z) / level_frame(band).dims);
    if (dot(gradient, gradient) <= 0.0) {
        return vec3<f32>(0.0);
    }
    var normal = normalize(-gradient);
    if (dot(normal, frame.light.xyz) < 0.0) {
        normal = -normal;
    }
    return normal;
}

fn march_baked(
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    direction: vec3<f32>,
    dither: f32,
) -> March {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var moment = 0.0;
    var mass = 0.0;
    var hit = vec4<f32>(0.0);
    var hit_normal = vec4<f32>(0.0);
    if (finish <= begin) {
        return March(radiance, transmittance, moment, mass, hit, hit_normal);
    }
    let extent = vec3<f32>(frame.atlas.xyz) * PADDED_SIDE;
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var complete = false;
    while (t < finish && samples < SAMPLE_LIMIT && cells < CELL_LIMIT && !complete) {
        let found = baked_reachable(origin, band_for(t, dither), t, direction);
        if (!found.usable) {
            let skip = ray_uvw(origin, found.band, t, direction);
            let rate = level_rate(found.band, direction);
            let edge = boundary_delta(found.band, skip, rate, local_step(t) * 0.25);
            t = min(t + edge, finish);
            continue;
        }
        let band = found.band;
        let g = level_frame(band);
        let direction_uvw = level_rate(band, direction);
        let brick = found.cell;
        let boundary = min(
            t + boundary_delta(band, found.uvw, direction_uvw, local_step(t) * 0.25),
            finish,
        );
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_slot(record) - 1u;
        let voxels_per_world = length(direction_uvw * g.dims);
        let texel_rate = direction_uvw * g.dims / extent;
        var texel = (slot_origin(frame.atlas.xyz, slot)
            + (clamp(found.uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims - brick_origin(brick))
            + vec3<f32>(1.0)) / extent;
        var cursor = t;
        while (cursor < boundary && cells < CELL_LIMIT && !complete) {
            cells += 1u;
            let step = local_step(cursor);
            let local = ray_uvw(origin, band, cursor, direction);
            let subcell = cell_index(cell_coord(cell_voxel(band, local, brick)));
            let limit = min(
                cursor + cell_delta(band, local, brick, direction_uvw, step * 0.25),
                boundary,
            );
            if (occupied_cell(slot, subcell) <= MINIMUM_TAU) {
                texel += texel_rate * (limit - cursor);
                cursor = limit;
                continue;
            }
            while (cursor < limit && samples < SAMPLE_LIMIT) {
                let sample = baked_sample(texel);
                let stride = clamp(
                    SEGMENT_ERROR / max(sample.a * voxels_per_world, 1.0e-9),
                    step,
                    step * SEGMENT_STRIDE,
                );
                let next = min(cursor + stride, limit);
                let segment = next - cursor;
                let tau = sample.a * segment * voxels_per_world;
                if (tau > 0.0) {
                    let alpha = 1.0 - exp(-tau);
                    let absorbed = transmittance * alpha;
                    radiance += absorbed * sample.rgb;
                    moment += absorbed * cursor;
                    mass += absorbed;
                    transmittance *= 1.0 - alpha;
                    if (hit.w <= 0.0 && tau >= SHADING_TAU) {
                        hit = vec4<f32>(level_world(band, ray_uvw(origin, band, cursor, direction)), 1.0);
                        hit_normal = vec4<f32>(baked_normal(texel, band, extent), 1.0);
                    }
                }
                texel += texel_rate * segment;
                cursor = next;
                samples += 1u;
                if (transmittance < MINIMUM_TRANSMITTANCE) {
                    complete = true;
                }
            }
        }
        t = max(cursor, boundary);
    }
    return March(radiance, transmittance, moment, mass, hit, hit_normal);
}
