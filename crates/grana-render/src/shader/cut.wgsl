struct Style {
    color: vec4<f32>,
    flags: vec4<f32>,
    surface: vec4<f32>,
}

@group(0) @binding(0) var layer: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;
@group(0) @binding(2) var identities: texture_2d<f32>;
@group(1) @binding(0) var<storage, read> styles: array<Style>;

const OUTLINE_ALPHA: f32 = 0.85;
const OUTLINE_RADIUS: i32 = 1;
const SELECTED: vec3<f32> = vec3<f32>(1.0, 0.86, 0.45);
const SELECTED_MIX: f32 = 0.55;

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let corner = corners[index];
    var output: Output;
    output.clip_position = vec4<f32>(corner, 0.0, 1.0);
    output.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return output;
}

@fragment
fn fs_composite(input: Output) -> @location(0) vec4<f32> {
    return textureSampleLevel(layer, layer_sampler, input.uv, 0.0);
}

fn decode_identity(value: vec4<f32>) -> u32 {
    let bytes = vec4<u32>(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5);
    return bytes.x | (bytes.y << 8u) | (bytes.z << 16u);
}

fn nearest_identity(coordinate: vec2<i32>) -> u32 {
    let extent = vec2<i32>(textureDimensions(identities));
    let at = clamp(coordinate, vec2<i32>(0), extent - vec2<i32>(1));
    return decode_identity(textureLoad(identities, at, 0));
}

@fragment
fn fs_outline(input: Output) -> @location(0) vec4<f32> {
    let coordinate = vec2<i32>(input.clip_position.xy);
    let near = nearest_identity(coordinate);
    if (near == 0u) {
        return vec4<f32>(0.0);
    }
    let offsets = array<vec2<i32>, 4>(
        vec2<i32>(OUTLINE_RADIUS, 0),
        vec2<i32>(-OUTLINE_RADIUS, 0),
        vec2<i32>(0, OUTLINE_RADIUS),
        vec2<i32>(0, -OUTLINE_RADIUS),
    );
    var boundary = false;
    for (var index = 0; index < 4; index += 1) {
        if (nearest_identity(coordinate + offsets[index]) < near) {
            boundary = true;
        }
    }
    if (!boundary) {
        return vec4<f32>(0.0);
    }
    let style = styles[near - 1u];
    let selected = style.flags.y > 0.5;
    let color = mix(style.color.rgb, SELECTED, select(0.0, SELECTED_MIX, selected));
    let alpha = select(OUTLINE_ALPHA, 1.0, selected);
    return vec4<f32>(color * alpha, alpha);
}
