fn outside_frustum(low: vec3<f32>, high: vec3<f32>) -> bool {
    for (var index = 0u; index < 6u; index += 1u) {
        let plane = frame.planes[index];
        let positive = vec3<f32>(
            select(low.x, high.x, plane.x >= 0.0),
            select(low.y, high.y, plane.y >= 0.0),
            select(low.z, high.z, plane.z >= 0.0),
        );
        if (dot(plane.xyz, positive) + plane.w < 0.0) {
            return true;
        }
    }
    return false;
}
