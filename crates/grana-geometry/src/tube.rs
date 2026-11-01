use std::f32::consts::TAU;

use glam::Vec3;

use crate::MeshVertex;

pub const TUBE_SIDES: usize = 8;

const SPHERE_RINGS: usize = 4;
const EPSILON: f32 = 1.0e-12;

pub fn push_tube(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    points: &[[f32; 3]],
    radii: &[f32],
    object: u32,
) {
    assert!(points.len() >= 2, "a tube follows at least two nodes");
    assert_eq!(
        points.len(),
        radii.len(),
        "a tube carries one radius per node"
    );
    assert!(
        points
            .iter()
            .flatten()
            .chain(radii)
            .all(|value| value.is_finite()),
        "a tube is finite"
    );
    assert!(
        radii.iter().all(|radius| *radius > 0.0),
        "a tube radius is a positive length"
    );
    assert!(
        points
            .windows(2)
            .all(|pair| { Vec3::from(pair[1]).distance_squared(Vec3::from(pair[0])) > EPSILON }),
        "a tube follows distinct nodes"
    );
    let points = points
        .iter()
        .map(|point| Vec3::from(*point))
        .collect::<Vec<_>>();
    let tangents = tangents(&points);
    let mut frame = basis(tangents[0]);
    let mut rings = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            frame = transport(frame, tangents[index - 1], tangents[index]);
        }
        let base = vertices.len() as u32;
        for side in 0..TUBE_SIDES {
            let angle = TAU * side as f32 / TUBE_SIDES as f32;
            let normal = frame.0 * angle.cos() + frame.1 * angle.sin();
            vertices.push(MeshVertex::new(
                (*point + normal * radii[index]).to_array(),
                normal.to_array(),
                object,
            ));
        }
        rings.push(base);
    }
    for pair in rings.windows(2) {
        connect(indices, pair[0], pair[1]);
    }
    let start = cap(vertices, points[0], -tangents[0], radii[0], object);
    for side in 0..TUBE_SIDES as u32 {
        let next = (side + 1) % TUBE_SIDES as u32;
        indices.extend_from_slice(&[start, rings[0] + next, rings[0] + side]);
    }
    let last = points.len() - 1;
    let end = cap(vertices, points[last], tangents[last], radii[last], object);
    for side in 0..TUBE_SIDES as u32 {
        let next = (side + 1) % TUBE_SIDES as u32;
        indices.extend_from_slice(&[end, rings[last] + side, rings[last] + next]);
    }
}

pub fn push_sphere(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    center: [f32; 3],
    radius: f32,
    object: u32,
) {
    assert!(
        center.iter().all(|value| value.is_finite()),
        "a sphere center is finite"
    );
    assert!(
        radius.is_finite() && radius > 0.0,
        "a sphere radius is a positive length"
    );
    let center = Vec3::from(center);
    let north = vertices.len() as u32;
    vertices.push(MeshVertex::new(
        (center + Vec3::Z * radius).to_array(),
        Vec3::Z.to_array(),
        object,
    ));
    let mut rings = Vec::with_capacity(SPHERE_RINGS - 1);
    for ring in 1..SPHERE_RINGS {
        let polar = std::f32::consts::PI * ring as f32 / SPHERE_RINGS as f32;
        let base = vertices.len() as u32;
        for side in 0..TUBE_SIDES {
            let azimuth = TAU * side as f32 / TUBE_SIDES as f32;
            let normal = Vec3::new(
                polar.sin() * azimuth.cos(),
                polar.sin() * azimuth.sin(),
                polar.cos(),
            );
            vertices.push(MeshVertex::new(
                (center + normal * radius).to_array(),
                normal.to_array(),
                object,
            ));
        }
        rings.push(base);
    }
    let south = vertices.len() as u32;
    vertices.push(MeshVertex::new(
        (center - Vec3::Z * radius).to_array(),
        (-Vec3::Z).to_array(),
        object,
    ));
    for side in 0..TUBE_SIDES as u32 {
        let next = (side + 1) % TUBE_SIDES as u32;
        indices.extend_from_slice(&[north, rings[0] + side, rings[0] + next]);
    }
    for pair in rings.windows(2) {
        connect(indices, pair[0], pair[1]);
    }
    let last = rings.len() - 1;
    for side in 0..TUBE_SIDES as u32 {
        let next = (side + 1) % TUBE_SIDES as u32;
        indices.extend_from_slice(&[south, rings[last] + next, rings[last] + side]);
    }
}

fn connect(indices: &mut Vec<u32>, lower: u32, upper: u32) {
    for side in 0..TUBE_SIDES as u32 {
        let next = (side + 1) % TUBE_SIDES as u32;
        indices.extend_from_slice(&[
            lower + side,
            upper + side,
            upper + next,
            lower + side,
            upper + next,
            lower + next,
        ]);
    }
}

fn cap(vertices: &mut Vec<MeshVertex>, point: Vec3, normal: Vec3, radius: f32, object: u32) -> u32 {
    let index = vertices.len() as u32;
    vertices.push(MeshVertex::new(
        (point + normal * radius).to_array(),
        normal.to_array(),
        object,
    ));
    index
}

fn tangents(points: &[Vec3]) -> Vec<Vec3> {
    let mut tangents = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        let direction = if index == 0 {
            points[1] - *point
        } else if index + 1 == points.len() {
            *point - points[index - 1]
        } else {
            points[index + 1] - points[index - 1]
        };
        let tangent = if direction.length_squared() > EPSILON {
            direction.normalize()
        } else if index > 0 {
            tangents[index - 1]
        } else {
            Vec3::Z
        };
        tangents.push(tangent);
    }
    tangents
}

fn basis(tangent: Vec3) -> (Vec3, Vec3) {
    let helper = if tangent.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let u = tangent.cross(helper).normalize();
    (u, tangent.cross(u))
}

fn transport(frame: (Vec3, Vec3), from: Vec3, to: Vec3) -> (Vec3, Vec3) {
    let axis = from.cross(to);
    let sine = axis.length();
    let rotated = if sine > 1.0e-6 {
        rotate(
            frame.0,
            axis / sine,
            sine.atan2(from.dot(to).clamp(-1.0, 1.0)),
        )
    } else {
        frame.0
    };
    let projected = rotated - to * rotated.dot(to);
    let u = if projected.length_squared() > EPSILON {
        projected.normalize()
    } else {
        basis(to).0
    };
    (u, to.cross(u))
}

fn rotate(vector: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let (sine, cosine) = angle.sin_cos();
    vector * cosine + axis.cross(vector) * sine + axis * axis.dot(vector) * (1.0 - cosine)
}
