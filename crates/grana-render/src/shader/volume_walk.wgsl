const WALK_EPSILON: f32 = 1.0e-6;

struct Sight {
    band: u32,
    cell: vec3<u32>,
    slot: u32,
    usable: bool,
    missing: bool,
    voxel: vec3<f32>,
}

struct Walk {
    sight: Sight,
    entry: f32,
    exit: f32,
    voxel: vec3<f32>,
    rate: vec3<f32>,
    base: vec3<f32>,
    far: vec3<f32>,
    texel: TexelMap,
    voxels: f32,
}

fn level_sight(origin: vec3<f32>, band: u32, t: f32, direction: vec3<f32>) -> Sight {
    let g = level_frame(band);
    let voxel = voxel_at(voxel_ray(band, origin, direction), t);
    let cell = brick_of(g.grid, g.dims, voxel);
    let record = level_record(band, brick_index(g.grid, cell));
    let slot = brick_display_slot(record);
    var usable = false;
    if (slot != 0u) {
        usable = occupied_brick(slot - 1u);
    }
    return Sight(band, cell, slot, usable, slot == 0u && !brick_hollow(record), voxel);
}

fn volume_reachable(
    origin: vec3<f32>,
    band: u32,
    hint: u32,
    t: f32,
    direction: vec3<f32>,
) -> Sight {
    let count = band_count();
    if (hint != band && hint < count) {
        let held = level_sight(origin, hint, t, direction);
        if (held.usable) {
            return held;
        }
    }
    let first = level_sight(origin, band, t, direction);
    if (first.usable) {
        return first;
    }
    var chosen = first;
    for (var step = 1u; step < count; step += 1u) {
        let coarser = band + step;
        if (coarser < count) {
            let other = level_sight(origin, coarser, t, direction);
            if (other.usable) {
                return other;
            }
            if (chosen.missing && !other.missing) {
                chosen = other;
            }
        }
        if (step <= band) {
            let finer = band - step;
            let other = level_sight(origin, finer, t, direction);
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

fn inside_level(sight: Sight) -> bool {
    let dims = level_frame(sight.band).dims;
    return all(sight.voxel >= vec3<f32>(-1.0))
        && all(sight.voxel <= dims + vec3<f32>(1.0));
}

fn walk_open(direction: vec3<f32>, begin: f32, finish: f32, sight: Sight) -> Walk {
    let g = level_frame(sight.band);
    let rate = level_rate(sight.band, direction) * g.dims;
    let base = brick_origin(sight.cell);
    let far = min(base + vec3<f32>(BRICK_SIDE), g.dims);
    let exit = box_exit(base, far, sight.voxel, rate);
    var texel = TexelMap(vec3<f32>(0.0), vec3<f32>(1.0));
    if (sight.usable) {
        texel = texel_map(sight.slot - 1u, base);
    }
    return Walk(
        sight,
        begin,
        min(begin + max(exit, max(begin, 1.0) * WALK_EPSILON), finish),
        sight.voxel,
        rate,
        base,
        far,
        texel,
        length(rate),
    );
}

fn walk_voxel(walk: Walk, t: f32) -> vec3<f32> {
    return walk.voxel + walk.rate * (t - walk.entry);
}

fn walk_cell(walk: Walk, voxel: vec3<f32>) -> vec3<u32> {
    return cell_coord(voxel - walk.base);
}

fn walk_cell_span(walk: Walk, coord: vec3<u32>, voxel: vec3<f32>) -> f32 {
    return cell_span(walk.base, coord, walk.far, voxel, walk.rate);
}

fn walk_texel(walk: Walk, voxel: vec3<f32>) -> vec3<f32> {
    return texel_at(walk.texel, voxel);
}
