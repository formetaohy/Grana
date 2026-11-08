@group(3) @binding(0) var object_depth: texture_depth_2d;
@group(4) @binding(0) var shadow_tau: texture_3d<f32>;
@group(4) @binding(1) var shadow_sampler: sampler;
@group(4) @binding(2) var shadow_occlusion: texture_3d<f32>;
@group(4) @binding(3) var shadow_objects: texture_depth_2d;
@group(4) @binding(4) var shadow_compare: sampler_comparison;

@group(5) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(5) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(5) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(5) @binding(3) var oit_accumulate: texture_2d<f32>;
@group(5) @binding(4) var oit_reveal: texture_2d<f32>;
@group(5) @binding(5) var oit_tail_depth: texture_2d<f32>;

const MAX_SAMPLES: u32 = 4096u;
const MAX_CELLS: u32 = 4096u;
const STRIDE_LIMIT: f32 = 8.0;
const SAMPLING_ERROR: f32 = 0.02;
const MINIMUM_TRANSMITTANCE: f32 = 0.003;
const MINIMUM_TAU: f32 = 1.0e-4;
const AMBIENT_OCCLUSION: f32 = 0.55;
const SHADING_FLOOR: f32 = 0.02;
const SHADING_CEIL: f32 = 0.10;
const EMPTY_DISTANCE: f32 = 1.0e19;
const MAXIMUM_ENTRIES: u32 = 9u;

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

fn object_distance(depth: f32, ndc: vec2<f32>) -> f32 {
    if (depth >= 1.0) {
        return EMPTY_DISTANCE;
    }
    return length(unproject(vec3<f32>(ndc, depth)) - frame.eye.xyz);
}

fn region_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    if (!region_active()) {
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    var entry = -1.0e20;
    var exit = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        let low = frame.region_min[axis];
        let high = frame.region_max[axis];
        if (abs(step) < 1.0e-9) {
            if (origin[axis] < low || origin[axis] > high) {
                return vec2<f32>(1.0, -1.0);
            }
        } else {
            let inverse = 1.0 / step;
            let first = (low - origin[axis]) * inverse;
            let second = (high - origin[axis]) * inverse;
            entry = max(entry, min(first, second));
            exit = min(exit, max(first, second));
        }
    }
    return vec2<f32>(entry, exit);
}

fn lighting(band: u32, uvw: vec3<f32>, normal: vec3<f32>) -> vec2<f32> {
    let occlusion = local_occlusion(band_uvw(band, uvw));
    let strength = clamp(frame.march.y, 0.0, 1.0);
    if (strength <= 0.0) {
        return vec2<f32>(1.0, occlusion);
    }
    let g = level_frame(band);
    let world = g.box_min + uvw * g.box_size;
    let coordinate = shadow_coordinate(frame.light_view, world);
    let sun = sun_transmittance(
        coordinate,
        shadow_bias(frame.light_reach, normal, frame.light.xyz),
    );
    return vec2<f32>(mix(1.0, sun, strength), occlusion);
}

fn lit(color: vec3<f32>, normal: vec3<f32>, direction: vec3<f32>, light: vec2<f32>) -> vec3<f32> {
    if (length(normal) < 0.5) {
        return color;
    }
    let facing = select(normal, -normal, dot(normal, direction) > 0.0);
    let half_vector = normalize(frame.light.xyz - direction);
    let diffuse = max(dot(normal, frame.light.xyz), 0.0) * light.x;
    let specular = pow(max(dot(facing, half_vector), 0.0), frame.shading.w) * light.x;
    let ambient = frame.shading.x * (1.0 - AMBIENT_OCCLUSION * light.y);
    return color * (ambient + frame.shading.y * diffuse) + vec3<f32>(frame.shading.z * specular);
}

fn shade_channel(
    band: u32,
    gradient: vec3<f32>,
    color: vec3<f32>,
    uvw: vec3<f32>,
    direction: vec3<f32>,
    voxel: vec3<f32>,
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
    let normal = shaded_normal(gradient, voxel);
    return mix(color, lit(color, normal, direction, lighting(band, uvw, normal)), weight);
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

fn cell_visible(band: u32, slot: u32, cell: u32, channels_active: u32) -> bool {
    let planes = (channels_active + PACK - 1u) / PACK;
    for (var plane = 0u; plane < planes; plane += 1u) {
        let maxima = level_maxima(band, slot, cell, plane);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = plane * PACK + component;
            if (channel >= channels_active) {
                break;
            }
            if (channel_opacity(channel) <= 0.0) {
                continue;
            }
            if (maxima[component] >= channel_threshold(channel)) {
                return true;
            }
        }
    }
    return false;
}

fn cell_extinction(band: u32, slot: u32, cell: u32, channels_active: u32) -> f32 {
    let planes = (channels_active + PACK - 1u) / PACK;
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
            extinction = max(extinction, channel_tau(channel, maxima[component]).a * opacity);
        }
    }
    return extinction;
}

fn sight(band: u32, t: f32, direction: vec3<f32>, channels_active: u32) -> BrickSight {
    let g = level_frame(band);
    let uvw = ray_uvw(band, t, direction);
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

fn reachable(band: u32, t: f32, direction: vec3<f32>, channels_active: u32) -> BrickSight {
    let count = band_count();
    let first = sight(band, t, direction, channels_active);
    if (first.usable) {
        return first;
    }
    var chosen = first;
    for (var step = 1u; step < count; step += 1u) {
        let coarser = band + step;
        if (coarser < count) {
            let other = sight(coarser, t, direction, channels_active);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
        if (step <= band) {
            let finer = band - step;
            let other = sight(finer, t, direction, channels_active);
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
    return max(frame.march.x, 0.5 * frame.jitter.w * t / frame.march.w);
}

fn march(begin: f32, finish: f32, direction: vec3<f32>, dither: f32) -> vec4<f32> {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    if (finish <= begin) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let channel_count = u32(frame.march.z);
    let pack_count = (channel_count + PACK - 1u) / PACK;
    let atlas_extent = vec3<f32>(frame.atlas.xyz) * PADDED_SIDE;
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var complete = false;
    while (t < finish && samples < MAX_SAMPLES && cells < MAX_CELLS && !complete) {
        var found = reachable(band_for(t, dither), t, direction, channel_count);
        let band = found.band;
        if (!found.usable) {
            let skip = ray_uvw(band, t, direction);
            let g = level_frame(band);
            let edge = boundary_delta(band, skip, direction / g.box_size, local_step(t) * 0.25);
            t = min(t + edge, finish);
            continue;
        }
        let g = level_frame(band);
        let direction_uvw = direction / g.box_size;
        let brick = found.cell;
        let uvw = found.uvw;
        let boundary = min(t + boundary_delta(band, uvw, direction_uvw, local_step(t) * 0.25), finish);
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_slot(record) - 1u;
        let voxel = g.box_size / g.dims;
        let voxels_per_world = length(direction / voxel);
        let gradient_step = vec3<f32>(1.0) / g.dims;
        let texel_rate = direction_uvw * g.dims / atlas_extent;
        var texel =
            (slot_origin(frame.atlas.xyz, slot)
                + (clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims - brick_origin(brick))
                + vec3<f32>(1.0)) / atlas_extent;
        var cursor = t;
        while (cursor < boundary && cells < MAX_CELLS && !complete) {
            cells += 1u;
            let step = local_step(cursor);
            let local = ray_uvw(band, cursor, direction);
            let subcell = cell_index(cell_coord(cell_voxel(band, local, brick)));
            let limit = min(cursor + cell_delta(band, local, brick, direction_uvw, step * 0.25), boundary);
            if (!cell_visible(band, slot, subcell, channel_count)) {
                texel += texel_rate * (limit - cursor);
                cursor = limit;
                continue;
            }
            let extinction = cell_extinction(band, slot, subcell, channel_count);
            let stride = clamp(
                SAMPLING_ERROR / max(extinction * voxels_per_world, 1.0e-9),
                step,
                step * STRIDE_LIMIT,
            );
            while (cursor < limit && samples < MAX_SAMPLES) {
                let next = min(cursor + stride, limit);
                let segment = next - cursor;
                let position = ray_uvw(band, cursor, direction);
                var optical_depth = 0.0;
                var emission = vec3<f32>(0.0);
                for (var pack = 0u; pack < pack_count; pack += 1u) {
                    if (!pack_active(pack, channel_count)) {
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
                        let extinction = channel_tau(channel, values[component]);
                        tau[component] = extinction.a;
                        color[component] = extinction.rgb;
                        shaded = shaded || extinction.a > SHADING_TAU;
                    }
                    if (shaded) {
                        let gradient = pack_gradient(band, pack, position, gradient_step);
                        for (var component = 0u; component < PACK; component += 1u) {
                            let channel = pack * PACK + component;
                            if (channel >= channel_count) {
                                break;
                            }
                            if (tau[component] <= SHADING_TAU || channel_opacity(channel) <= 0.0) {
                                continue;
                            }
                            color[component] = shade_channel(
                                band,
                                gradient[component],
                                color[component],
                                position,
                                direction,
                                voxel,
                            );
                        }
                    }
                    for (var component = 0u; component < PACK; component += 1u) {
                        let channel = pack * PACK + component;
                        if (channel >= channel_count) {
                            break;
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
                    radiance += transmittance * (1.0 - segment_transmittance) * average_color;
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
    return vec4<f32>(radiance, transmittance);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let eye = frame.eye.xyz;
    let ndc = input.ndc + frame.jitter.xy;
    let pixel = vec2<i32>(input.clip_position.xy);
    let direction = normalize(unproject(vec3<f32>(ndc, 1.0)) - eye);
    let box_min = frame.box_min.xyz;
    let box_max = box_min + frame.box_size.xyz;

    let inverse_direction = vec3<f32>(1.0) / direction;
    let near_t = (box_min - eye) * inverse_direction;
    let far_t = (box_max - eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(textureLoad(object_depth, pixel, 0), ndc);
    let region = region_interval(eye, direction);
    let plane = clip_interval(eye, direction);
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
        let local = pixel - origin;
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
    for (var index = 0u; index < layers; index += 1u) {
        let limit = min(object_distance(layer_depth[index], ndc), exit);
        if (limit > cursor) {
            let segment = march(cursor, limit, direction, dither);
            radiance += transmittance * segment.rgb;
            transmittance *= segment.a;
            cursor = limit;
        }
        let sample = vec4<f32>(
            unpack2x16float(layer_color[index].x),
            unpack2x16float(layer_color[index].y),
        );
        radiance += transmittance * sample.rgb;
        transmittance *= 1.0 - sample.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    if (cursor < exit) {
        let segment = march(cursor, exit, direction, dither);
        radiance += transmittance * segment.rgb;
        transmittance *= segment.a;
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
