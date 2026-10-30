use std::f32::consts::TAU;

use glam::Vec3;
use grana_object::{Mesh, MeshVertex, TrackSet};

const SIDES: usize = 8;
const EPSILON: f32 = 1.0e-12;

pub fn track_mesh(set: &TrackSet, radius: f32) -> Mesh {
    assert!(
        radius.is_finite() && radius > 0.0,
        "a track tube radius must be a positive length"
    );
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (index, track) in set.tracks().iter().enumerate() {
        let object = index as u32;
        let mut points = Vec::with_capacity(track.nodes().len() + 1);
        if let Some(parent) = track.parent() {
            let parent = set.track(parent as usize).expect("the parent track exists");
            let end = parent.nodes().last().expect("the parent holds nodes");
            points.push(Vec3::from(end.centroid()));
        }
        points.extend(track.nodes().iter().map(|node| Vec3::from(node.centroid())));
        deduplicate(&mut points);
        match points.len() {
            0 => {}
            1 => sphere(&mut vertices, &mut indices, points[0], radius, object),
            _ => tube(&mut vertices, &mut indices, &points, radius, object),
        }
    }
    Mesh::new(vertices, indices)
}

fn deduplicate(points: &mut Vec<Vec3>) {
    let mut unique = Vec::with_capacity(points.len());
    for point in points.drain(..) {
        if unique
            .last()
            .is_none_or(|last: &Vec3| last.distance_squared(point) > EPSILON)
        {
            unique.push(point);
        }
    }
    *points = unique;
}

fn tube(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    points: &[Vec3],
    radius: f32,
    object: u32,
) {
    let tangents = tangents(points);
    let mut basis = basis(tangents[0]);
    let mut rings = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            basis = transport(basis, tangents[index - 1], tangents[index]);
        }
        let base = vertices.len() as u32;
        for side in 0..SIDES {
            let angle = TAU * side as f32 / SIDES as f32;
            let normal = basis.0 * angle.cos() + basis.1 * angle.sin();
            vertices.push(MeshVertex::new(
                (*point + normal * radius).to_array(),
                normal.to_array(),
                object,
            ));
        }
        rings.push(base);
    }
    for pair in rings.windows(2) {
        connect(indices, pair[0], pair[1]);
    }
    let start = cap(vertices, points[0], -tangents[0], object);
    for side in 0..SIDES as u32 {
        let next = (side + 1) % SIDES as u32;
        indices.extend_from_slice(&[start, rings[0] + next, rings[0] + side]);
    }
    let last = points.len() - 1;
    let end = cap(vertices, points[last], tangents[last], object);
    for side in 0..SIDES as u32 {
        let next = (side + 1) % SIDES as u32;
        indices.extend_from_slice(&[end, rings[last] + side, rings[last] + next]);
    }
}

fn connect(indices: &mut Vec<u32>, lower: u32, upper: u32) {
    for side in 0..SIDES as u32 {
        let next = (side + 1) % SIDES as u32;
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

fn cap(vertices: &mut Vec<MeshVertex>, point: Vec3, normal: Vec3, object: u32) -> u32 {
    let index = vertices.len() as u32;
    vertices.push(MeshVertex::new(point.to_array(), normal.to_array(), object));
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

fn sphere(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    center: Vec3,
    radius: f32,
    object: u32,
) {
    const AXES: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let base = vertices.len() as u32;
    for axis in AXES {
        let normal = Vec3::from(axis);
        vertices.push(MeshVertex::new(
            (center + normal * radius).to_array(),
            axis,
            object,
        ));
    }
    indices.extend_from_slice(&[
        base,
        base + 2,
        base + 4,
        base,
        base + 4,
        base + 3,
        base,
        base + 5,
        base + 2,
        base,
        base + 3,
        base + 5,
        base + 1,
        base + 4,
        base + 2,
        base + 1,
        base + 3,
        base + 4,
        base + 1,
        base + 2,
        base + 5,
        base + 1,
        base + 5,
        base + 3,
    ]);
}
