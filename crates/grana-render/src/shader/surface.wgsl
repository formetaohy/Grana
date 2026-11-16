const PROBE_REACH: f32 = 1.0e19;
const PROBE_EPSILON: f32 = 1.0e-4;

struct CellHit {
    found: bool,
    missing: bool,
    cell: vec3<u32>,
    entry: f32,
    exit: f32,
}

fn probe_cell(
    band: u32,
    origin: vec3<f32>,
    direction: vec3<f32>,
    begin: f32,
    finish: f32,
) -> CellHit {
    var hit = CellHit(false, false, vec3<u32>(0u), 0.0, 0.0);
    let g = level_frame(band);
    let ray = voxel_ray(band, origin, direction);
    let ceiling = CELL_GRID * (g.grid.x + g.grid.y + g.grid.z);
    var held = vec3<u32>(0xffffffffu);
    var slot = 0u;
    var t = begin;
    var steps = 0u;
    while (t < finish && steps < ceiling) {
        steps += 1u;
        let voxel = voxel_at(ray, t);
        if (any(voxel < vec3<f32>(-1.0)) || any(voxel > g.dims + vec3<f32>(1.0))) {
            break;
        }
        let brick = brick_of(g.grid, g.dims, voxel);
        if (any(brick != held)) {
            held = brick;
            let record = level_record(band, brick_index(g.grid, brick));
            slot = brick_display_slot(record);
            hit.missing = hit.missing || (slot == 0u && !brick_hollow(record));
            if (slot != 0u && !occupied_brick(slot - 1u)) {
                slot = 0u;
            }
        }
        let base = brick_origin(brick);
        let far = min(base + vec3<f32>(BRICK_SIDE), g.dims);
        let coord = cell_coord(voxel - base);
        if (slot != 0u && occupied_cell(slot - 1u, cell_index(coord))) {
            hit.found = true;
            hit.cell = brick * vec3<u32>(CELL_GRID) + coord;
            hit.entry = t;
            hit.exit = t + cell_span(base, coord, far, voxel, ray.rate);
            return hit;
        }
        let span = select(
            box_exit(base, far, voxel, ray.rate),
            cell_span(base, coord, far, voxel, ray.rate),
            slot != 0u,
        );
        if (span >= PROBE_REACH) {
            break;
        }
        t += max(span, max(t, 1.0) * PROBE_EPSILON);
    }
    return hit;
}

fn probe_normal(band: u32, cell: vec3<u32>) -> vec3<f32> {
    let g = level_frame(band);
    let brick = cell / vec3<u32>(CELL_GRID);
    let slot = brick_display_slot(level_record(band, brick_index(g.grid, brick))) - 1u;
    let center = (vec3<f32>(cell) + vec3<f32>(0.5)) * CELL_SIDE;
    return structure_normal(texel_at(texel_map(slot, brick_origin(brick)), center));
}

fn probe_surface(
    origin: vec3<f32>,
    direction: vec3<f32>,
    begin: f32,
    finish: f32,
) -> vec4<f32> {
    var chosen = CellHit(false, false, vec3<u32>(0u), 0.0, 0.0);
    var chosen_band = 0u;
    var span = vec2<f32>(begin, finish);
    var descended = false;
    var band = band_count();
    while (band > 0u) {
        band -= 1u;
        let found = probe_cell(band, origin, direction, span.x, span.y);
        if (found.found) {
            chosen = found;
            chosen_band = band;
            span = vec2<f32>(found.entry, found.exit);
            descended = true;
        } else if (descended || !found.missing) {
            break;
        }
    }
    if (!chosen.found) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let normal = probe_normal(chosen_band, chosen.cell);
    if (dot(normal, normal) <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return vec4<f32>(normal, clip_depth(origin + direction * chosen.entry));
}

@fragment
fn fs_surface(input: VertexOutput) -> @location(0) vec4<f32> {
    let ndc = input.ndc + frame.jitter.xy;
    if (slice_active()) {
        let ray = camera_ray(ndc);
        let crossing = slice_crossing(ray.origin, ray.direction);
        let depth = select(1.0, clip_depth(crossing.xyz), crossing.w >= 0.5);
        return vec4<f32>(0.0, 0.0, 0.0, depth);
    }
    let resolution = resolve(vec2<i32>(floor(input.clip_position.xy)), ndc);
    return probe_surface(
        resolution.origin,
        resolution.direction,
        resolution.start,
        resolution.exit,
    );
}
