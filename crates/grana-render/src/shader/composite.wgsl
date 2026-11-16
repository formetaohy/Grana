const SEGMENT_ERROR: f32 = 0.02;
const SEGMENT_STRIDE: f32 = 8.0;

struct March {
    radiance: vec3<f32>,
    transmittance: f32,
}

struct Travel {
    walk: Walk,
    held: u32,
    occupied: bool,
    samples: u32,
    cells: u32,
}

struct Journey {
    travel: Travel,
    march: March,
}

fn travel_open(
    origin: vec3<f32>,
    begin: f32,
    reach: f32,
    direction: vec3<f32>,
    dither: f32,
    hint: u32,
    taken: vec2<u32>,
) -> Travel {
    let band = band_for(begin, dither);
    let sight = volume_reachable(origin, band, hint, begin, direction);
    return Travel(walk_open(direction, begin, reach, sight), 0xffffffffu, true, taken.x, taken.y);
}

fn journey_begin(
    origin: vec3<f32>,
    begin: f32,
    reach: f32,
    direction: vec3<f32>,
    dither: f32,
) -> Journey {
    let travel = travel_open(
        origin,
        begin,
        reach,
        direction,
        dither,
        band_count(),
        vec2<u32>(0u, 0u),
    );
    return Journey(travel, March(vec3<f32>(0.0), 1.0));
}

fn march_layer(march: March, layer: vec4<f32>) -> March {
    return March(
        march.radiance + march.transmittance * layer.rgb,
        march.transmittance * (1.0 - layer.a),
    );
}

fn journey_march(
    journey: Journey,
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    reach: f32,
    direction: vec3<f32>,
    dither: f32,
) -> Journey {
    var travel = journey.travel;
    var march = journey.march;
    if (finish <= begin) {
        return journey;
    }
    let resumed = begin < travel.walk.exit
        && travel.walk.entry <= begin
        && band_for(begin, dither) == travel.walk.sight.band;
    if (!resumed) {
        let hint = travel.walk.sight.band;
        travel = travel_open(
            origin,
            begin,
            reach,
            direction,
            dither,
            hint,
            vec2<u32>(travel.samples, travel.cells),
        );
    }
    var t = begin;
    while (t < finish && travel.samples < frame.bounds.x && travel.cells < frame.bounds.y) {
        if (!inside_level(travel.walk.sight)) {
            break;
        }
        if (t >= travel.walk.exit) {
            let hint = travel.walk.sight.band;
            travel = travel_open(
                origin,
                t,
                reach,
                direction,
                dither,
                hint,
                vec2<u32>(travel.samples, travel.cells),
            );
            continue;
        }
        if (!travel.walk.sight.usable) {
            travel.cells += 1u;
            t = travel.walk.exit;
            continue;
        }
        let slot = travel.walk.sight.slot - 1u;
        var cursor = t;
        let limit = min(travel.walk.exit, finish);
        while (cursor < limit && travel.samples < frame.bounds.x && travel.cells < frame.bounds.y) {
            travel.cells += 1u;
            let stride = local_step(cursor);
            let voxel = walk_voxel(travel.walk, cursor);
            let coord = walk_cell(travel.walk, voxel);
            let cell = cell_index(coord);
            if (cell != travel.held) {
                travel.held = cell;
                travel.occupied = occupied_cell(slot, cell);
            }
            if (!travel.occupied) {
                let cell_reach = walk_cell_span(travel.walk, coord, voxel);
                let gap = stride * ceil(max(cell_reach, 0.0) / stride);
                cursor = min(cursor + max(gap, stride), limit);
                continue;
            }
            let texel = walk_texel(travel.walk, voxel);
            let sample = optical_sample(texel);
            let advance = clamp(
                SEGMENT_ERROR / max(sample.a * travel.walk.voxels, 1.0e-9),
                stride,
                stride * SEGMENT_STRIDE,
            );
            let segment = min(advance, limit - cursor);
            let tau = sample.a * segment * travel.walk.voxels;
            if (tau > 0.0) {
                let alpha = 1.0 - exp(-tau);
                march.radiance += march.transmittance
                    * alpha
                    * structure_lighting(sample.rgb, texel, origin + direction * cursor);
                march.transmittance *= 1.0 - alpha;
            }
            cursor += segment;
            travel.samples += 1u;
            if (march.transmittance < MINIMUM_TRANSMITTANCE) {
                return Journey(travel, march);
            }
        }
        t = max(cursor, t);
    }
    return Journey(travel, march);
}
