const PROBE_REACH: f32 = 1.0e19;
const PROBE_EPSILON: f32 = 1.0e-4;

struct CellHit {
    found: bool,
    missing: bool,
    cell: vec3<u32>,
    entry: f32,
    exit: f32,
}

fn level_cell(band: u32, uvw: vec3<f32>) -> vec3<u32> {
    let g = level_frame(band);
    let voxel = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims;
    let last = vec3<f32>(g.grid * CELL_GRID) - vec3<f32>(1.0);
    return vec3<u32>(clamp(floor(voxel / CELL_SIDE), vec3<f32>(0.0), last));
}

fn box_span(low: vec3<f32>, high: vec3<f32>, uvw: vec3<f32>, rate: vec3<f32>) -> f32 {
    var span = PROBE_REACH;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = rate[axis];
        if (abs(step) > 1.0e-9) {
            let boundary = select(low[axis], high[axis], step >= 0.0);
            span = min(span, (boundary - uvw[axis]) / step);
        }
    }
    return clamp(span, 0.0, PROBE_REACH);
}

fn cell_span(
    band: u32,
    cell: vec3<u32>,
    uvw: vec3<f32>,
    rate: vec3<f32>,
    brick: vec3<u32>,
) -> f32 {
    let g = level_frame(band);
    let side = CELL_SIDE * f32(CELL_GRID);
    let near = max(
        vec3<f32>(brick) * side / g.dims,
        vec3<f32>(cell) * CELL_SIDE / g.dims,
    );
    let far = min(
        (vec3<f32>(brick) + vec3<f32>(1.0)) * side / g.dims,
        (vec3<f32>(cell) + vec3<f32>(1.0)) * CELL_SIDE / g.dims,
    );
    return box_span(near, far, uvw, rate);
}

fn brick_span(band: u32, brick: vec3<u32>, uvw: vec3<f32>, rate: vec3<f32>) -> f32 {
    let g = level_frame(band);
    let side = CELL_SIDE * f32(CELL_GRID);
    let low = vec3<f32>(brick) * side / g.dims;
    let high = (vec3<f32>(brick) + vec3<f32>(1.0)) * side / g.dims;
    return box_span(low, high, uvw, rate);
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
    let rate = level_rate(band, direction);
    let ceiling = CELL_GRID * (g.grid.x + g.grid.y + g.grid.z);
    var held = vec3<u32>(0xffffffffu);
    var slot = 0u;
    var t = begin;
    var steps = 0u;
    while (t < finish && steps < ceiling) {
        steps += 1u;
        let raw = ray_uvw(origin, band, t, direction);
        if (any(raw < vec3<f32>(-PROBE_EPSILON)) || any(raw > vec3<f32>(1.0 + PROBE_EPSILON))) {
            break;
        }
        let uvw = clamp(raw, vec3<f32>(0.0), vec3<f32>(1.0));
        let cell = level_cell(band, uvw);
        let brick = cell / vec3<u32>(CELL_GRID);
        if (any(brick != held)) {
            held = brick;
            let record = level_record(band, brick_index(g.grid, brick));
            slot = brick_display_slot(record);
            hit.missing = hit.missing || (slot == 0u && !brick_hollow(record));
            if (slot != 0u && !occupied_brick(slot - 1u)) {
                slot = 0u;
            }
        }
        if (slot != 0u) {
            let local = cell - brick * vec3<u32>(CELL_GRID);
            if (occupied_cell(slot - 1u, cell_index(local))) {
                hit.found = true;
                hit.cell = cell;
                hit.entry = t;
                hit.exit = t + cell_span(band, cell, uvw, rate, brick);
                return hit;
            }
        }
        let span = select(
            brick_span(band, brick, uvw, rate),
            cell_span(band, cell, uvw, rate, brick),
            slot != 0u,
        );
        if (span >= PROBE_REACH) {
            break;
        }
        t += max(span, max(t, 1.0) * 1.0e-6);
    }
    return hit;
}

fn probe_normal(band: u32, cell: vec3<u32>) -> vec3<f32> {
    let g = level_frame(band);
    let brick = cell / vec3<u32>(CELL_GRID);
    let slot = brick_display_slot(level_record(band, brick_index(g.grid, brick))) - 1u;
    let center = (vec3<f32>(cell) + vec3<f32>(0.5)) * CELL_SIDE / g.dims;
    return structure_normal(brick_texel(band, center, brick, slot));
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
    let resolution = resolve(input.clip_position, ndc);
    return probe_surface(
        resolution.origin,
        resolution.direction,
        resolution.start,
        resolution.exit,
    );
}

