@group(3) @binding(0) var object_depth: texture_depth_2d;
@group(4) @binding(0) var shadow_tau: texture_3d<f32>;
@group(4) @binding(1) var shadow_sampler: sampler;
@group(4) @binding(3) var shadow_objects: texture_depth_2d;
@group(4) @binding(4) var shadow_compare: sampler_comparison;

@group(5) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(5) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(5) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(5) @binding(3) var oit_accumulate: texture_2d<f32>;
@group(5) @binding(4) var oit_reveal: texture_2d<f32>;
@group(5) @binding(5) var oit_tail_depth: texture_2d<f32>;

@group(6) @binding(0) var ao_map: texture_2d<f32>;

const MAX_SAMPLES: u32 = 4096u;
const MAX_CELLS: u32 = 4096u;
const STRIDE_LIMIT: f32 = 8.0;
const SAMPLING_ERROR: f32 = 0.02;
const MINIMUM_TRANSMITTANCE: f32 = 0.003;
const MINIMUM_TAU: f32 = 1.0e-4;
const OCCLUSION_STRENGTH: f32 = 0.55;
const SHADING_FLOOR: f32 = 0.02;
const SHADING_CEIL: f32 = 0.10;
const VOLUME_ROUGHNESS: f32 = 0.85;
const VOLUME_METALLIC: f32 = 0.0;
const EMPTY_DISTANCE: f32 = 1.0e19;
const MAXIMUM_ENTRIES: u32 = 9u;
const CHANNEL_LIMIT: u32 = 16u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

struct BrickSight {
    usable: bool,
    missing: bool,
    band: u32,
    cell: vec3<u32>,
    uvw: vec3<f32>,
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

fn object_distance(origin: vec3<f32>, depth: f32, ndc: vec2<f32>) -> f32 {
    if (depth >= 1.0) {
        return EMPTY_DISTANCE;
    }
    return length(unproject(vec3<f32>(ndc, depth)) - origin);
}

fn region_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    if (!region_active()) {
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    let offset = origin - frame.region_center.xyz;
    var entry = -1.0e20;
    var exit = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let unit = frame.region_axis[axis].xyz;
        let half = frame.region_axis[axis].w;
        let start = dot(offset, unit);
        let step = dot(direction, unit);
        if (abs(step) < 1.0e-9) {
            if (abs(start) > half) {
                return vec2<f32>(1.0, -1.0);
            }
        } else {
            let inverse = 1.0 / step;
            let first = (-half - start) * inverse;
            let second = (half - start) * inverse;
            entry = max(entry, min(first, second));
            exit = min(exit, max(first, second));
        }
    }
    return vec2<f32>(entry, exit);
}

fn lighting(band: u32, uvw: vec3<f32>, normal: vec3<f32>) -> f32 {
    let strength = clamp(frame.march.y, 0.0, 1.0);
    if (strength <= 0.0) {
        return 1.0;
    }
    let world = level_world(band, uvw);
    let coordinate = shadow_coordinate(frame.light_view, world);
    return sun_transmittance(
        coordinate,
        shadow_bias(frame.light_reach, normal, frame.light.xyz),
    );
}

fn lit(
    color: vec3<f32>,
    normal: vec3<f32>,
    direction: vec3<f32>,
    light: f32,
    ao: f32,
) -> vec3<f32> {
    if (length(normal) < 0.5) {
        return color;
    }
    let facing = select(normal, -normal, dot(normal, direction) > 0.0);
    return shade_surface(
        color,
        VOLUME_ROUGHNESS,
        VOLUME_METALLIC,
        facing,
        -direction,
        frame.light.xyz,
        light,
        frame.shading.x,
        frame.shading.y,
        1.0,
    ) * (1.0 - OCCLUSION_STRENGTH * (1.0 - ao));
}

fn shade_channel(
    band: u32,
    gradient: vec3<f32>,
    color: vec3<f32>,
    uvw: vec3<f32>,
    direction: vec3<f32>,
    ao: f32,
) -> vec3<f32> {
    let magnitude = length(gradient);
    if (magnitude < GRADIENT_EPSILON) {
        return color;
    }
    let weight = clamp(
        (magnitude - SHADING_FLOOR) / (SHADING_CEIL - SHADING_FLOOR),
        0.0,
        1.0,
    );
    if (weight <= 0.0) {
        return color;
    }
    let normal = shaded_normal(gradient, band);
    return mix(color, lit(color, normal, direction, lighting(band, uvw, normal), ao), weight);
}

fn brick_visible(record: BrickRecord, channels_active: u32) -> bool {
    if (!brick_holds(record)) {
        return false;
    }
    for (var channel = 0u; channel < channels_active; channel += 1u) {
        if (channel_opacity(channel) <= 0.0) {
            continue;
        }
        let plane = channel / PACK;
        let component = channel - plane * PACK;
        if (record.maxima[plane][component] >= channel_threshold(channel)) {
            return true;
        }
    }
    return false;
}

fn cell_reach(band: u32, slot: u32, cell: u32, channels_active: u32) -> vec2<f32> {
    let planes = (channels_active + PACK - 1u) / PACK;
    var visible = 0.0;
    var extinction = 0.0;
    for (var plane = 0u; plane < planes; plane += 1u) {
        let maxima = level_maxima(band, slot, cell, plane);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = plane * PACK + component;
            if (channel >= channels_active) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0 || maxima[component] < channel_threshold(channel)) {
                continue;
            }
            visible = 1.0;
            extinction = max(extinction, channel_tau(channel, maxima[component]).a * opacity);
        }
    }
    return vec2<f32>(visible, extinction);
}

fn sight(
    origin: vec3<f32>,
    band: u32,
    t: f32,
    direction: vec3<f32>,
    channels_active: u32,
) -> BrickSight {
    let g = level_frame(band);
    let uvw = ray_uvw(origin, band, t, direction);
    let cell = brick_cell(g.grid, g.dims, uvw);
    let record = level_record(band, brick_index(g.grid, cell));
    let slot = brick_slot(record);
    let hollow = brick_hollow(record);
    var usable = slot != 0u && brick_visible(record, channels_active);
    if (slot != 0u && record.flags == 0u) {
        usable = false;
    }
    return BrickSight(usable, slot == 0u && !hollow, band, cell, uvw);
}

fn reachable(
    origin: vec3<f32>,
    band: u32,
    t: f32,
    direction: vec3<f32>,
    channels_active: u32,
) -> BrickSight {
    let count = band_count();
    let first = sight(origin, band, t, direction, channels_active);
    if (first.usable) {
        return first;
    }
    var chosen = first;
    for (var step = 1u; step < count; step += 1u) {
        let coarser = band + step;
        if (coarser < count) {
            let other = sight(origin, coarser, t, direction, channels_active);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
        if (step <= band) {
            let finer = band - step;
            let other = sight(origin, finer, t, direction, channels_active);
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

fn local_step(t: f32) -> f32 {
    return max(frame.march.x, 0.5 * frame.jitter.w * pixel_world(t));
}

fn refined_modes(channels_active: u32) -> bool {
    for (var channel = 0u; channel < channels_active; channel += 1u) {
        if (channel_opacity(channel) > 0.0 && channel_mode(channel) > 0.5) {
            return true;
        }
    }
    return false;
}

struct March {
    radiance: vec3<f32>,
    transmittance: f32,
    hit: vec4<f32>,
    normal: vec4<f32>,
    moment: f32,
    mass: f32,
    peak: f32,
    peak_distance: f32,
}

fn march(
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    direction: vec3<f32>,
    dither: f32,
    ao: f32,
) -> March {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var hit = vec4<f32>(0.0);
    var hit_normal = vec4<f32>(0.0);
    var moment = 0.0;
    var mass = 0.0;
    var brightest = -1.0;
    var brightest_distance = 0.0;
    if (finish <= begin) {
        return March(
            radiance,
            transmittance,
            hit,
            hit_normal,
            moment,
            mass,
            brightest,
            brightest_distance,
        );
    }
    let channel_count = u32(frame.march.z);
    let pack_count = (channel_count + PACK - 1u) / PACK;
    let atlas_extent = vec3<f32>(frame.atlas.xyz) * PADDED_SIDE;
    let refined = refined_modes(channel_count);
    var peak = array<f32, CHANNEL_LIMIT>();
    for (var channel = 0u; channel < CHANNEL_LIMIT; channel += 1u) {
        peak[channel] = -1.0;
    }
    var pack_mask = 0u;
    for (var pack = 0u; pack < pack_count; pack += 1u) {
        if (pack_active(pack, channel_count)) {
            pack_mask |= 1u << pack;
        }
    }
    var finished = 0u;
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var complete = false;
    while (t < finish && samples < MAX_SAMPLES && cells < MAX_CELLS && !complete) {
        var found = reachable(origin, band_for(t, dither), t, direction, channel_count);
        let band = found.band;
        if (!found.usable) {
            let skip = ray_uvw(origin, band, t, direction);
            let g = level_frame(band);
            let edge = boundary_delta(band, skip, level_rate(band, direction), local_step(t) * 0.25);
            t = min(t + edge, finish);
            continue;
        }
        let g = level_frame(band);
        let direction_uvw = level_rate(band, direction);
        let brick = found.cell;
        let uvw = found.uvw;
        let boundary = min(t + boundary_delta(band, uvw, direction_uvw, local_step(t) * 0.25), finish);
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_slot(record) - 1u;
        let voxels_per_world = length(direction_uvw * g.dims);
        let texel_rate = direction_uvw * g.dims / atlas_extent;
        var texel =
            (slot_origin(frame.atlas.xyz, slot)
                + (clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims - brick_origin(brick))
                + vec3<f32>(1.0)) / atlas_extent;
        var cursor = t;
        while (cursor < boundary && cells < MAX_CELLS && !complete) {
            cells += 1u;
            let step = local_step(cursor);
            let local = ray_uvw(origin, band, cursor, direction);
            let subcell = cell_index(cell_coord(cell_voxel(band, local, brick)));
            let limit = min(cursor + cell_delta(band, local, brick, direction_uvw, step * 0.25), boundary);
            let reach = cell_reach(band, slot, subcell, channel_count);
            if (reach.x <= 0.5) {
                texel += texel_rate * (limit - cursor);
                cursor = limit;
                continue;
            }
            let extinction = reach.y;
            let adaptive = clamp(
                SAMPLING_ERROR / max(extinction * voxels_per_world, 1.0e-9),
                step,
                step * STRIDE_LIMIT,
            );
            let stride = select(adaptive, step, refined);
            while (cursor < limit && samples < MAX_SAMPLES) {
                let next = min(cursor + stride, limit);
                let segment = next - cursor;
                let position = ray_uvw(origin, band, cursor, direction);
                var optical_depth = 0.0;
                var emission = vec3<f32>(0.0);
                for (var pack = 0u; pack < pack_count; pack += 1u) {
                    if ((pack_mask & (1u << pack)) == 0u) {
                        continue;
                    }
                    let values = sample_atlas(pack, texel);
                    var tau = array<f32, PACK>(0.0, 0.0, 0.0, 0.0);
                    var color = array<vec3<f32>, PACK>(
                        vec3<f32>(0.0),
                        vec3<f32>(0.0),
                        vec3<f32>(0.0),
                        vec3<f32>(0.0),
                    );
                    var iso = array<bool, PACK>(false, false, false, false);
                    var shaded = false;
                    for (var component = 0u; component < PACK; component += 1u) {
                        let channel = pack * PACK + component;
                        if (channel >= channel_count) {
                            break;
                        }
                        if (channel_opacity(channel) <= 0.0
                            || values[component] < channel_threshold(channel)) {
                            continue;
                        }
                        if (channel_isosurface(channel)) {
                            if ((finished & (1u << channel)) != 0u) {
                                continue;
                            }
                            iso[component] = true;
                            color[component] = channel_tau(channel, values[component]).rgb;
                            shaded = true;
                            continue;
                        }
                        if (channel_maximum(channel)) {
                            peak[channel] = max(peak[channel], values[component]);
                            if (values[component] > brightest) {
                                brightest = values[component];
                                brightest_distance = cursor;
                            }
                            continue;
                        }
                        let extinction = channel_tau(channel, values[component]);
                        tau[component] = extinction.a;
                        color[component] = extinction.rgb;
                        shaded = shaded || extinction.a > SHADING_TAU;
                    }
                    if (shaded) {
                        let gradient = texel_gradient(pack, texel, values, atlas_extent);
                        if (hit.w <= 0.0) {
                            for (var component = 0u; component < PACK; component += 1u) {
                                let channel = pack * PACK + component;
                                if (channel >= channel_count) {
                                    break;
                                }
                                if (channel_opacity(channel) <= 0.0) {
                                    continue;
                                }
                                if (!iso[component] && tau[component] <= SHADING_TAU) {
                                    continue;
                                }
                                let surface_normal = gradient[component];
                                if (length(surface_normal) < GRADIENT_EPSILON) {
                                    continue;
                                }
                                hit = vec4<f32>(level_world(band, position), 1.0);
                                hit_normal = vec4<f32>(shaded_normal(surface_normal, band), 1.0);
                                break;
                            }
                        }
                        for (var component = 0u; component < PACK; component += 1u) {
                            let channel = pack * PACK + component;
                            if (channel >= channel_count) {
                                break;
                            }
                            if (channel_opacity(channel) <= 0.0) {
                                continue;
                            }
                            if (!iso[component] && tau[component] <= SHADING_TAU) {
                                continue;
                            }
                            color[component] = shade_channel(
                                band,
                                gradient[component],
                                color[component],
                                position,
                                direction,
                                ao,
                            );
                            if (iso[component]) {
                                let alpha = clamp(channel_opacity(channel), 0.0, 1.0);
                                radiance += transmittance * alpha * color[component];
                                transmittance *= 1.0 - alpha;
                                finished |= 1u << channel;
                            }
                        }
                    }
                    for (var component = 0u; component < PACK; component += 1u) {
                        let channel = pack * PACK + component;
                        if (channel >= channel_count) {
                            break;
                        }
                        if (iso[component]) {
                            continue;
                        }
                        let opacity = channel_opacity(channel);
                        let weight_tau = tau[component];
                        if (opacity <= 0.0 || weight_tau <= MINIMUM_TAU) {
                            continue;
                        }
                        let weight = weight_tau * opacity * segment * voxels_per_world;
                        optical_depth += weight;
                        emission += weight * color[component];
                    }
                }
                if (optical_depth > 0.0) {
                    let segment_transmittance = exp(-optical_depth);
                    let average_color = emission / optical_depth;
                    let absorbed = transmittance * (1.0 - segment_transmittance);
                    moment += absorbed * cursor;
                    mass += absorbed;
                    radiance += absorbed * average_color;
                    transmittance *= segment_transmittance;
                }
                texel += texel_rate * segment;
                cursor = next;
                samples += 1u;
                if (transmittance < MINIMUM_TRANSMITTANCE) {
                    complete = true;
                    break;
                }
            }
        }
        t = max(cursor, boundary);
    }
    for (var channel = 0u; channel < channel_count; channel += 1u) {
        if (!channel_maximum(channel) || peak[channel] < 0.0) {
            continue;
        }
        let opacity = channel_opacity(channel);
        if (opacity <= 0.0) {
            continue;
        }
        let color = channel_tau(channel, peak[channel]).rgb;
        radiance += transmittance * opacity * color;
        transmittance *= 1.0 - opacity;
    }
    return March(
        radiance,
        transmittance,
        hit,
        hit_normal,
        moment,
        mass,
        brightest,
        brightest_distance,
    );
}

struct VolumeOutput {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

fn ambient_occlusion(pixel: vec2<f32>) -> f32 {
    let size = vec2<i32>(frame.viewport.zw);
    let at = vec2<i32>(floor(volume_target_pixel(pixel) - frame.viewport.xy));
    if (at.x < 0 || at.y < 0 || at.x >= size.x || at.y >= size.y) {
        return 1.0;
    }
    return textureLoad(ao_map, at, 0).r;
}

fn clip_depth(world: vec3<f32>) -> f32 {
    let clip = frame.view_projection * vec4<f32>(world, 1.0);
    return clamp(clip.z / clip.w, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> VolumeOutput {
    let ndc = input.ndc + frame.jitter.xy;
    let scene_pixel = vec2<i32>(floor(volume_target_pixel(input.clip_position.xy)));
    let ao = ambient_occlusion(input.clip_position.xy);
    let ray = camera_ray(ndc);
    let origin = ray.origin;
    let direction = ray.direction;
    let local_eye = level_local(0u, origin);
    let local_direction = level_rate(0u, direction);

    let inverse_direction = vec3<f32>(1.0) / local_direction;
    let near_t = (vec3<f32>(0.0) - local_eye) * inverse_direction;
    let far_t = (vec3<f32>(1.0) - local_eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(origin, textureLoad(object_depth, scene_pixel, 0), ndc);
    let region = region_interval(origin, direction);
    let plane = clip_interval(origin, direction);
    let clip = vec2<f32>(max(region.x, plane.x), min(region.y, plane.y));
    let exit = min(
        min(min(highest_t.x, highest_t.y), highest_t.z),
        min(min(object, clip.y), 1.0e19),
    );
    let start = max(max(entry, 0.0), clip.x);
    let jitter = sample_noise(input.clip_position.xy, frame.jitter.z) * local_step(start);
    let dither = sample_noise(input.clip_position.xy, frame.jitter.z + 8.0);

    var layer_depth: array<f32, MAXIMUM_ENTRIES>;
    var layer_color: array<vec2<u32>, MAXIMUM_ENTRIES>;
    var layers = 0u;
    var tail = vec4<f32>(0.0);
    var tail_depth = EMPTY_DISTANCE;
    if (frame.oit.x > 0u) {
        let origin = vec2<i32>(frame.viewport.xy);
        let size = vec2<i32>(frame.viewport.zw);
        let local = scene_pixel - origin;
        if (local.x >= 0 && local.y >= 0 && local.x < size.x && local.y < size.y) {
            let index = u32(local.y) * u32(size.x) + u32(local.x);
            let base = index * frame.oit.y;
            let stored = min(oit_counts[index], frame.oit.y);
            for (var slot = 0u; slot < stored; slot += 1u) {
                let key = oit_keys[base + slot];
                if (key == 0u) {
                    break;
                }
                layer_depth[layers] = bitcast<f32>(~key);
                layer_color[layers] = oit_colors[base + slot];
                layers += 1u;
            }
            let scale = max(frame.oit.w, 1u);
            let tail_coord = (vec2<f32>(local) + 0.5) / f32(scale) - 0.5;
            let tail_base = vec2<i32>(floor(tail_coord));
            let tail_fraction = tail_coord - floor(tail_coord);
            let tail_extent = vec2<i32>(textureDimensions(oit_accumulate)) - vec2<i32>(1);
            let low = clamp(tail_base, vec2<i32>(0), tail_extent);
            let high = clamp(tail_base + vec2<i32>(1), vec2<i32>(0), tail_extent);
            let across = vec2<i32>(high.x, low.y);
            let down = vec2<i32>(low.x, high.y);
            let accumulation = mix(
                mix(
                    textureLoad(oit_accumulate, low, 0),
                    textureLoad(oit_accumulate, across, 0),
                    tail_fraction.x,
                ),
                mix(
                    textureLoad(oit_accumulate, down, 0),
                    textureLoad(oit_accumulate, high, 0),
                    tail_fraction.x,
                ),
                tail_fraction.y,
            );
            if (accumulation.a > 0.0) {
                let reveal = mix(
                    mix(
                        textureLoad(oit_reveal, low, 0).r,
                        textureLoad(oit_reveal, across, 0).r,
                        tail_fraction.x,
                    ),
                    mix(
                        textureLoad(oit_reveal, down, 0).r,
                        textureLoad(oit_reveal, high, 0).r,
                        tail_fraction.x,
                    ),
                    tail_fraction.y,
                );
                let nearest = min(
                    min(
                        textureLoad(oit_tail_depth, low, 0).x,
                        textureLoad(oit_tail_depth, across, 0).x,
                    ),
                    min(
                        textureLoad(oit_tail_depth, down, 0).x,
                        textureLoad(oit_tail_depth, high, 0).x,
                    ),
                );
                tail = vec4<f32>(accumulation.rgb / accumulation.a, 1.0 - reveal);
                if (nearest < 1.0) {
                    tail_depth = nearest;
                }
            }
        }
    }

    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    if (tail.a > 0.0) {
        var position = tail_depth;
        if (layers > 0u) {
            position = max(position, layer_depth[layers - 1u]);
        }
        let premultiplied = vec4<f32>(tail.rgb * tail.a, tail.a);
        layer_depth[layers] = position;
        layer_color[layers] = vec2<u32>(
            pack2x16float(premultiplied.rg),
            pack2x16float(premultiplied.ba),
        );
        layers += 1u;
    }
    var cursor = start + min(jitter, max(exit - start, 0.0));
    var hit = vec4<f32>(0.0);
    var hit_normal = vec4<f32>(0.0);
    var moment = 0.0;
    var mass = 0.0;
    var brightest = -1.0;
    var brightest_distance = 0.0;
    for (var index = 0u; index < layers; index += 1u) {
        let limit = min(object_distance(origin, layer_depth[index], ndc), exit);
        if (limit > cursor) {
            let segment = march(origin, cursor, limit, direction, dither, ao);
            moment += transmittance * segment.moment;
            mass += transmittance * segment.mass;
            if (segment.peak > brightest) {
                brightest = segment.peak;
                brightest_distance = segment.peak_distance;
            }
            radiance += transmittance * segment.radiance;
            transmittance *= segment.transmittance;
            if (hit.w <= 0.0 && segment.hit.w > 0.0) {
                hit = segment.hit;
                hit_normal = segment.normal;
            }
            cursor = limit;
        }
        let sample = vec4<f32>(
            unpack2x16float(layer_color[index].x),
            unpack2x16float(layer_color[index].y),
        );
        let absorbed = transmittance * sample.a;
        moment += absorbed * layer_depth[index];
        mass += absorbed;
        radiance += transmittance * sample.rgb;
        transmittance *= 1.0 - sample.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    if (cursor < exit) {
        let segment = march(origin, cursor, exit, direction, dither, ao);
        moment += transmittance * segment.moment;
        mass += transmittance * segment.mass;
        if (segment.peak > brightest) {
            brightest = segment.peak;
            brightest_distance = segment.peak_distance;
        }
        radiance += transmittance * segment.radiance;
        transmittance *= segment.transmittance;
        if (hit.w <= 0.0 && segment.hit.w > 0.0) {
            hit = segment.hit;
            hit_normal = segment.normal;
        }
    }
    var depth = 1.0;
    if (hit.w > 0.0) {
        depth = clip_depth(hit.xyz);
    } else if (mass > 1.0e-4) {
        depth = clip_depth(origin + direction * (moment / mass));
    } else if (brightest >= 0.0) {
        depth = clip_depth(origin + direction * brightest_distance);
    }
    return VolumeOutput(
        vec4<f32>(radiance, 1.0 - transmittance),
        vec4<f32>(hit_normal.xyz, depth),
    );
}
