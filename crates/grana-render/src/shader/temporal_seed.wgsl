struct SeedData {
    previous: mat4x4<f32>,
    previous_inverse: mat4x4<f32>,
    flags: vec4<f32>,
}

@group(2) @binding(0) var<uniform> seed: SeedData;
@group(2) @binding(1) var history: texture_2d<f32>;
@group(2) @binding(2) var history_geometry: texture_2d<f32>;
@group(2) @binding(3) var fresh: texture_2d<f32>;

const NORMAL_LIMIT: f32 = 0.9;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.clip = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

struct Seeded {
    @location(0) color: vec4<f32>,
    @location(1) count: f32,
}

fn inverse_project(inverse: mat4x4<f32>, ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = inverse * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
}

fn history_color(uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32> {
    let low = vec2<i32>(bounds.xy);
    let high = vec2<i32>(bounds.zw);
    let coord = uv - vec2<f32>(0.5);
    let base = floor(coord);
    let weight = coord - base;
    let corner = vec2<i32>(base);
    let across = clamp(corner + vec2<i32>(1, 0), low, high);
    let down = clamp(corner + vec2<i32>(0, 1), low, high);
    let diagonal = clamp(corner + vec2<i32>(1, 1), low, high);
    let left = clamp(corner, low, high);
    let first = mix(
        textureLoad(history, left, 0),
        textureLoad(history, across, 0),
        weight.x,
    );
    let second = mix(
        textureLoad(history, down, 0),
        textureLoad(history, diagonal, 0),
        weight.x,
    );
    return mix(first, second, weight.y);
}

struct Neighbourhood {
    low: vec4<f32>,
    high: vec4<f32>,
}

fn neighbourhood(surface: vec2<i32>, bounds: vec4<f32>) -> Neighbourhood {
    let low = vec2<i32>(bounds.xy);
    let high = vec2<i32>(bounds.zw);
    var lowest = vec4<f32>(1.0e20);
    var highest = vec4<f32>(-1.0e20);
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let at = clamp(surface + vec2<i32>(x, y), low, high);
            let value = textureLoad(fresh, at, 0);
            lowest = min(lowest, value);
            highest = max(highest, value);
        }
    }
    return Neighbourhood(lowest, highest);
}

@fragment
fn fs_main(input: VertexOutput) -> Seeded {
    let surface = vec2<i32>(input.clip.xy);
    let origin = frame.viewport.xy;
    let size = frame.viewport.zw;
    let bounds = vec4<f32>(origin, origin + size - vec2<f32>(1.0));
    let geometry = camera_surface(surface, input.ndc);
    var uv = input.clip.xy;
    var valid = true;
    if (seed.flags.y < 0.5) {
        if (!geometry.known) {
            valid = false;
        } else {
            let world = unproject(vec3<f32>(input.ndc, geometry.depth));
            let clip = seed.previous * vec4<f32>(world, 1.0);
            let ndc = clip.xy / clip.w;
            uv = origin + (ndc * 0.5 + 0.5) * size;
            if (any(uv < bounds.xy) || any(uv > bounds.zw)) {
                valid = false;
            } else {
                let stored = surface_of(textureLoad(history_geometry, vec2<i32>(round(uv)), 0));
                if (!stored.known) {
                    valid = false;
                } else {
                    let previous = inverse_project(seed.previous_inverse, vec3<f32>(ndc, stored.depth));
                    if (distance(previous, world) > seed.flags.z) {
                        valid = false;
                    } else if (
                        length(stored.normal) >= NORMAL_REACH
                        && length(geometry.normal) >= NORMAL_REACH
                        && dot(normalize(geometry.normal), normalize(stored.normal)) < NORMAL_LIMIT
                    ) {
                        valid = false;
                    }
                }
            }
        }
    }
    if (!valid) {
        return Seeded(vec4<f32>(0.0), 0.0);
    }
    let limits = neighbourhood(surface, bounds);
    let motion = clamp(length(uv - input.clip.xy) / max(seed.flags.w, 1.0), 0.0, 1.0);
    let weight = mix(seed.flags.x, 1.0, motion);
    let color = clamp(history_color(uv, bounds), limits.low, limits.high);
    return Seeded(color * weight, weight);
}

