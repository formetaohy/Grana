const SEGMENT_ERROR: f32 = 0.02;
const SEGMENT_STRIDE: f32 = 8.0;

struct March {
    radiance: vec3<f32>,
    transmittance: f32,
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
    if (finish <= begin) {
        return March(radiance, transmittance);
    }
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var walk = walk_open(
        direction,
        begin,
        finish,
        volume_reachable(origin, band_for(begin, dither), band_count(), begin, direction),
    );
    while (t < finish && samples < frame.bounds.x && cells < frame.bounds.y) {
        if (!inside_level(walk.sight)) {
            break;
        }
        if (t >= walk.exit) {
            let sight = volume_reachable(origin, band_for(t, dither), walk.sight.band, t, direction);
            walk = walk_open(direction, t, finish, sight);
            continue;
        }
        if (!walk.sight.usable) {
            cells += 1u;
            t = walk.exit;
            continue;
        }
        let slot = walk.sight.slot - 1u;
        var held = 0xffffffffu;
        var occupied = true;
        var cursor = t;
        while (cursor < walk.exit && samples < frame.bounds.x && cells < frame.bounds.y) {
            cells += 1u;
            let stride = local_step(cursor);
            let voxel = walk_voxel(walk, cursor);
            let coord = walk_cell(walk, voxel);
            let cell = cell_index(coord);
            if (cell != held) {
                held = cell;
                occupied = occupied_cell(slot, cell);
            }
            if (!occupied) {
                let reach = walk_cell_span(walk, coord, voxel);
                let gap = stride * ceil(max(reach, 0.0) / stride);
                cursor = min(cursor + max(gap, stride), walk.exit);
                continue;
            }
            let texel = walk_texel(walk, voxel);
            let sample = optical_sample(texel);
            let advance = clamp(
                SEGMENT_ERROR / max(sample.a * walk.voxels, 1.0e-9),
                stride,
                stride * SEGMENT_STRIDE,
            );
            let segment = min(advance, walk.exit - cursor);
            let tau = sample.a * segment * walk.voxels;
            if (tau > 0.0) {
                let alpha = 1.0 - exp(-tau);
                radiance += transmittance
                    * alpha
                    * structure_lighting(sample.rgb, texel, origin + direction * cursor);
                transmittance *= 1.0 - alpha;
            }
            cursor += segment;
            samples += 1u;
            if (transmittance < MINIMUM_TRANSMITTANCE) {
                return March(radiance, transmittance);
            }
        }
        t = walk.exit;
    }
    return March(radiance, transmittance);
}
