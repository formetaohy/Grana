struct ScreenUniforms {
    flags: vec4<f32>,
    fade: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> screen: ScreenUniforms;
@group(0) @binding(2) var previous: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
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
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn resolve(pixel: vec2<f32>) -> vec4<f32> {
    return textureLoad(source, vec2<i32>(pixel), 0);
}

fn earlier(pixel: vec2<f32>) -> vec4<f32> {
    return textureLoad(previous, vec2<i32>(pixel), 0);
}

@fragment
fn fs_accumulate(input: VertexOutput) -> @location(0) vec4<f32> {
    return resolve(input.clip_position.xy);
}

@fragment
fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.clip_position.xy;
    let weight = screen.fade.x;
    let fresh = resolve(pixel) * screen.flags.z;
    let stale = earlier(pixel) * screen.fade.y;
    let accumulated = mix(stale, fresh, weight);
    let coverage = clamp(accumulated.a, 0.0, 1.0);
    let noise = (dither(pixel) - 0.5) / 255.0;
    if screen.flags.w > 0.5 {
        let straight_new = srgb_encode(aces(fresh.rgb * screen.flags.y / max(fresh.a, 1.0e-4)));
        let straight_old = srgb_encode(aces(stale.rgb * screen.flags.y / max(stale.a, 1.0e-4)));
        let straight = mix(straight_old, straight_new, weight);
        let encoded = clamp(straight + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), coverage);
    }
    let straight = accumulated.rgb / max(accumulated.a, 1.0e-4);
    let mapped =
        aces(straight * screen.flags.y) * coverage + aces(background(input.ndc)) * (1.0 - coverage);
    let encoded = clamp(srgb_encode(mapped) + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), 1.0);
}
