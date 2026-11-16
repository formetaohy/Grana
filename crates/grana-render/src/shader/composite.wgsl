@group(5) @binding(0) var<storage, read> occupied: array<u32>;

const OCCUPANCY_WORDS: u32 = 65u;
const OCCUPANCY_PACKS: u32 = 4u;
const SEGMENT_ERROR: f32 = 0.02;
const SEGMENT_STRIDE: f32 = 8.0;

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

fn occupied_brick(slot: u32) -> bool {
    for (var pack = 0u; pack < OCCUPANCY_PACKS; pack += 1u) {
        let word = (slot * OCCUPANCY_PACKS + pack) * OCCUPANCY_WORDS;
        if (occupancy_reaches(occupied[word])) {
            return true;
        }
    }
    return false;
}

fn occupied_cell(slot: u32, cell: u32) -> bool {
    for (var pack = 0u; pack < OCCUPANCY_PACKS; pack += 1u) {
        let word = (slot * OCCUPANCY_PACKS + pack) * OCCUPANCY_WORDS + 1u + cell;
        if (occupancy_reaches(occupied[word])) {
            return true;
        }
    }
    return false;
}

fn volume_sight(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> Sight {
    let g = level_frame(band);
    let uvw = ray_uvw(origin, band, t, direction);
    let cell = brick_cell(g.grid, g.dims, uvw);
    let record = level_record(band, brick_index(g.grid, cell));
    let slot = brick_display_slot(record);
    var usable = false;
    if (slot != 0u) {
        usable = occupied_brick(slot - 1u);
    }
    return Sight(usable, slot == 0u && !brick_hollow(record), band, cell, uvw);
}

fn volume_reachable(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> Sight {
    let count = band_count();
    let first = volume_sight(origin, band, t, direction);
    if (first.usable) {
        return first;
    }
    var chosen = first;
    for (var step = 1u; step < count; step += 1u) {
        let coarser = band + step;
        if (coarser < count) {
            let other = volume_sight(origin, coarser, t, direction);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
        if (step <= band) {
            let finer = band - step;
            let other = volume_sight(origin, finer, t, direction);
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

fn march_volume(
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
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var complete = false;
    while (t < finish && samples < frame.bounds.x && cells < frame.bounds.y && !complete) {
        let found = volume_reachable(origin, band_for(t, dither), t, direction);
        let band = found.band;
        let direction_uvw = level_rate(band, direction);
        let cell_step = local_step(t);
        let exit = boundary_delta(band, found.uvw, direction_uvw);
        let boundary = min(t + max(exit, max(t, 1.0) * 1.0e-6), finish);
        if (!found.usable) {
            let gap = cell_step * ceil(max(boundary - t, 0.0) / cell_step);
            t = min(t + max(gap, cell_step), finish);
            cells += 1u;
            continue;
        }
        let g = level_frame(band);
        let brick = found.cell;
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_display_slot(record) - 1u;
        let voxels_per_world = length(direction_uvw * g.dims);
        var held = 0xffffffffu;
        var occupied = true;
        var cursor = t;
        while (cursor < boundary && samples < frame.bounds.x && cells < frame.bounds.y && !complete) {
            cells += 1u;
            let stride = local_step(cursor);
            let local = ray_uvw(origin, band, cursor, direction);
            let subcell = cell_index(cell_coord(cell_voxel(band, local, brick)));
            if (subcell != held) {
                held = subcell;
                occupied = occupied_cell(slot, subcell);
            }
            if (!occupied) {
                let cell_exit = max(cell_reach(band, local, brick, direction_uvw), 0.0);
                let gap = stride * ceil(cell_exit / stride);
                cursor = min(cursor + max(gap, stride), boundary);
                continue;
            }
            let texel = brick_texel(band, local, brick, slot);
            let sample = optical_sample(texel);
            let advance = clamp(
                SEGMENT_ERROR / max(sample.a * voxels_per_world, 1.0e-9),
                stride,
                stride * SEGMENT_STRIDE,
            );
            let segment = min(advance, boundary - cursor);
            let tau = sample.a * segment * voxels_per_world;
            if (tau > 0.0) {
                let alpha = 1.0 - exp(-tau);
                let absorbed = transmittance * alpha;
                radiance += absorbed * volume_lighting(sample.rgb, texel, origin + direction * cursor);
                moment += absorbed * cursor;
                mass += absorbed;
                transmittance *= 1.0 - alpha;
                if (hit.w <= 0.0 && tau >= SHADING_TAU) {
                    hit = vec4<f32>(level_world(band, local), 1.0);
                    hit_normal = vec4<f32>(structure_normal(texel), 1.0);
                }
            }
            cursor += segment;
            samples += 1u;
            if (transmittance < MINIMUM_TRANSMITTANCE) {
                complete = true;
            }
        }
        t = max(cursor, boundary);
    }
    return March(radiance, transmittance, moment, mass, hit, hit_normal);
}
