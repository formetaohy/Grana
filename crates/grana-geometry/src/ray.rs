use glam::Vec3;

use crate::PackedMesh;
use crate::bvh::{Primitive, box_span};

const UNIT: f32 = 65535.0;

pub fn ray_hit(mesh: &PackedMesh, origin: Vec3, direction: Vec3) -> Option<Vec3> {
    assert!(
        origin.is_finite() && direction.is_finite(),
        "a picking ray is finite"
    );
    assert!(
        direction.length_squared() > 0.0,
        "a picking ray follows a positive direction"
    );
    let direction = direction.normalize();
    let mut nearest = f32::INFINITY;
    let mut hit = None;
    for (object, group) in mesh.groups().iter().enumerate() {
        let low = Vec3::from(group.min());
        let high = low + Vec3::from(group.scale()) * UNIT;
        let Some((entry, exit)) = box_span(low, high, origin, direction) else {
            continue;
        };
        if exit < 0.0 || entry > nearest {
            continue;
        }
        for triangle in 0..group.index_count() as usize / 3 {
            let corners = mesh.triangle(object, triangle).map(Vec3::from);
            let Some(distance) = Primitive::triangle(corners).hit(origin, direction) else {
                continue;
            };
            if distance < nearest {
                nearest = distance;
                hit = Some(origin + direction * distance);
            }
        }
    }
    hit
}
