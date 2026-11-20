use glam::DVec3;

pub fn signed_volume(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> f64 {
    let mut sum = 0.0f64;
    for triangle in triangles {
        let [first, second, third] = triangle.map(|index| point(positions, index));
        sum += first.dot(second.cross(third));
    }
    sum / 6.0
}

pub fn enclosed_volume(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> f64 {
    signed_volume(positions, triangles).abs()
}

fn point(positions: &[[f32; 3]], index: u32) -> DVec3 {
    DVec3::from(positions[index as usize].map(f64::from))
}
