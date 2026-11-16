struct SurfaceOutput {
    @location(0) normal: vec4<f32>,
}

@fragment
fn fs_surface(input: VertexOutput) -> SurfaceOutput {
    let ndc = input.ndc + frame.jitter.xy;
    var output: SurfaceOutput;
    if (slice_active()) {
        let ray = camera_ray(ndc);
        let crossing = slice_crossing(ray.origin, ray.direction);
        let depth = select(1.0, clip_depth(crossing.xyz), crossing.w >= 0.5);
        output.normal = vec4<f32>(0.0, 0.0, 0.0, depth);
        return output;
    }
    let resolution = resolve(input.clip_position, ndc);
    let cursor = resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0));
    var hit = vec4<f32>(0.0);
    var normal = vec4<f32>(0.0);
    if (cursor < resolution.exit) {
        let segment = march_volume(
            resolution.origin,
            cursor,
            resolution.exit,
            resolution.direction,
            resolution.dither,
        );
        hit = segment.hit;
        normal = segment.normal;
    }
    let depth = select(1.0, clip_depth(hit.xyz), hit.w > 0.0);
    output.normal = vec4<f32>(normal.xyz, depth);
    return output;
}
