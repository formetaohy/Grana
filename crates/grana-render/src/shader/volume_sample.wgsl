@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 4>;
@group(2) @binding(1) var atlas_sampler: sampler;

const ISO_EXTINCTION: f32 = 8.0;
const ISO_SOFTNESS: f32 = 0.04;
const CELL_SIDE: f32 = 8.0;
const CELL_GRID: u32 = 4u;
const LEVEL_EXIT: f32 = 1.0e20;

struct VolumeFrame {
    dims: vec3<f32>,
    grid: vec3<u32>,
}

struct VoxelRay {
    start: vec3<f32>,
    rate: vec3<f32>,
}

fn volume_frame() -> VolumeFrame {
    return VolumeFrame(frame.dims.xyz, frame.grids.xyz);
}

fn volume_record(index: u32) -> BrickRecord {
    return bricks[frame.base.x + index];
}

fn voxel_ray(origin: vec3<f32>, direction: vec3<f32>) -> VoxelRay {
    let dims = frame.dims.xyz;
    return VoxelRay(volume_local(origin) * dims, volume_rate(direction) * dims);
}

fn voxel_at(ray: VoxelRay, t: f32) -> vec3<f32> {
    return ray.start + ray.rate * t;
}

fn box_exit(low: vec3<f32>, high: vec3<f32>, voxel: vec3<f32>, rate: vec3<f32>) -> f32 {
    var exit = LEVEL_EXIT;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = rate[axis];
        if (abs(step) > 1.0e-9) {
            let far = select(low[axis], high[axis], step > 0.0);
            exit = min(exit, (far - voxel[axis]) / step);
        }
    }
    return exit;
}

fn cell_coord(local: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(
        clamp(
            floor(local / CELL_SIDE),
            vec3<f32>(0.0),
            vec3<f32>(f32(CELL_GRID) - 1.0),
        ),
    );
}

fn cell_index(cell: vec3<u32>) -> u32 {
    return cell.x + CELL_GRID * (cell.y + CELL_GRID * cell.z);
}

fn cell_span(base: vec3<f32>, coord: vec3<u32>, far: vec3<f32>, voxel: vec3<f32>, rate: vec3<f32>) -> f32 {
    let low = base + vec3<f32>(coord) * CELL_SIDE;
    let high = min(low + vec3<f32>(CELL_SIDE), far);
    return box_exit(low, high, voxel, rate);
}

fn tap_blend(pack: u32, texel: vec3<u32>, local: vec3<f32>) -> vec4<f32> {
    let low = texel + vec3<u32>(floor(local));
    let weight = local - floor(local);
    let x00 = textureLoad(atlases[pack], low, 0);
    let x10 = textureLoad(atlases[pack], low + vec3<u32>(1u, 0u, 0u), 0);
    let x01 = textureLoad(atlases[pack], low + vec3<u32>(0u, 1u, 0u), 0);
    let x11 = textureLoad(atlases[pack], low + vec3<u32>(1u, 1u, 0u), 0);
    let y00 = textureLoad(atlases[pack], low + vec3<u32>(0u, 0u, 1u), 0);
    let y10 = textureLoad(atlases[pack], low + vec3<u32>(1u, 0u, 1u), 0);
    let y01 = textureLoad(atlases[pack], low + vec3<u32>(0u, 1u, 1u), 0);
    let y11 = textureLoad(atlases[pack], low + vec3<u32>(1u, 1u, 1u), 0);
    let near = mix(mix(x00, x10, weight.x), mix(x01, x11, weight.x), weight.y);
    let far = mix(mix(y00, y10, weight.x), mix(y01, y11, weight.x), weight.y);
    return mix(near, far, weight.z);
}

fn nearest_components(mask: u32) -> vec4<bool> {
    return vec4<bool>(
        (mask & 1u) != 0u,
        (mask & 2u) != 0u,
        (mask & 4u) != 0u,
        (mask & 8u) != 0u,
    );
}

fn tap_fetch(pack: u32, texel: vec3<u32>, local: vec3<f32>) -> vec4<f32> {
    let mask = pack_nearest(pack);
    if (mask == 0u) {
        return tap_blend(pack, texel, local);
    }
    let snapped = texel + vec3<u32>(floor(local + vec3<f32>(0.5)));
    let exact = textureLoad(atlases[pack], snapped, 0);
    if (mask == PACK_MASK) {
        return exact;
    }
    return select(tap_blend(pack, texel, local), exact, nearest_components(mask));
}

fn tap_sample(pack: u32, voxel: vec3<f32>) -> vec4<f32> {
    let g = volume_frame();
    let cell = brick_of(g.grid, g.dims, voxel);
    let record = volume_record(brick_index(g.grid, cell));
    let slot = brick_display_slot(record);
    if (slot == 0u) {
        return vec4<f32>(0.0);
    }
    let tap = brick_tap(slot - 1u, brick_origin(cell));
    return tap_fetch(pack, tap_atlas_texel(tap), tap_local(tap, voxel));
}