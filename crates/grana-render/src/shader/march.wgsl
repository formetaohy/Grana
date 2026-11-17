const SEGMENT_ERROR: f32 = 0.02;
const SEGMENT_STRIDE: f32 = 8.0;
const SURFACE_OPACITY: f32 = 0.1;

struct March {
    radiance: vec3<f32>,
    transmittance: f32,
    surface: vec2<f32>,
    normal: vec4<f32>,
}

struct VolumeOutput {
    @location(0) radiance: vec4<f32>,
    @location(1) surface: vec2<f32>,
    @location(2) normal: vec4<f32>,
}

fn march_volume(
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    direction: vec3<f32>,
    dither: f32,
    peaks: ptr<function, array<f32, CHANNEL_LIMIT>>,
) -> March {
    var march = March(vec3<f32>(0.0), 1.0, vec2<f32>(0.0, 1.0), vec4<f32>(0.0));
    if (finish <= begin) {
        return march;
    }
    var t = begin;
    var walk = walk_open(
        direction,
        begin,
        finish,
        volume_reachable(origin, band_for(begin, dither), band_count(), begin, direction),
    );
    var held = 0xffffffffu;
    var occupied = true;
    var samples = 0u;
    var cells = 0u;
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
        var cursor = t;
        let limit = min(walk.exit, finish);
        while (cursor < limit && samples < frame.bounds.x && cells < frame.bounds.y) {
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
                let cell_reach = walk_cell_span(walk, coord, voxel);
                let gap = stride * ceil(max(cell_reach, 0.0) / stride);
                cursor = min(cursor + max(gap, stride), limit);
                continue;
            }
            let texel = walk_texel(walk, voxel);
            let sample = optical_sample(texel, peaks);
            let ceiling = select(stride * SEGMENT_STRIDE, stride, projection());
            let advance = clamp(
                SEGMENT_ERROR / max(sample.extinction * walk.voxels, 1.0e-9),
                stride,
                ceiling,
            );
            let segment = min(advance, limit - cursor);
            let tau = sample.extinction * segment * walk.voxels;
            let alpha = 1.0 - exp(-tau);
            if (alpha > 0.0) {
                let world = origin + direction * cursor;
                let albedo = sample.emission / max(sample.extinction, OPTICAL_MINIMUM_TAU);
                march.radiance += march.transmittance * alpha * structure_lighting(albedo, structure_tap(texel), world);
                march.transmittance *= 1.0 - alpha;
            }
            if (march.surface.y >= 1.0
                && (1.0 - march.transmittance >= SURFACE_OPACITY || sample.peak)) {
                march.surface = vec2<f32>(cursor, clip_depth(origin + direction * cursor));
                march.normal = structure_sample_normal(structure_tap(texel));
            }
            cursor += segment;
            samples += 1u;
            if (march.transmittance < MINIMUM_TRANSMITTANCE) {
                return march;
            }
        }
        t = max(cursor, t);
    }
    return march;
}

fn march_peaks(march: March, peaks: array<f32, CHANNEL_LIMIT>) -> March {
    var result = march;
    for (var channel = 0u; channel < CHANNEL_LIMIT; channel += 1u) {
        if (!channel_peak(channel) || peaks[channel] < 0.0) {
            continue;
        }
        let opacity = channel_opacity(channel);
        result.radiance += result.transmittance * opacity * channel_tau(channel, peaks[channel]).rgb;
        result.transmittance *= 1.0 - opacity;
    }
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> VolumeOutput {
    let ndc = input.ndc + frame.jitter.xy;
    let pixel = vec2<i32>(input.clip_position.xy);
    let resolution = resolve(pixel, ndc);
    var peaks: array<f32, CHANNEL_LIMIT>;
    if (projection()) {
        for (var channel = 0u; channel < CHANNEL_LIMIT; channel += 1u) {
            peaks[channel] = -1.0;
        }
    }
    var march = march_volume(
        resolution.origin,
        resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0)),
        resolution.exit,
        resolution.direction,
        resolution.dither,
        &peaks,
    );
    if (projection()) {
        march = march_peaks(march, peaks);
    }
    return VolumeOutput(
        vec4<f32>(march.radiance, 1.0 - march.transmittance),
        march.surface,
        march.normal,
    );
}
