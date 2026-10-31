use glam::Vec3;
use grana_object::{Mesh, push_sphere, push_tube};

use grana_object::TrackSet;

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
            1 => push_sphere(
                &mut vertices,
                &mut indices,
                points[0].to_array(),
                radius,
                object,
            ),
            _ => {
                let positions = points
                    .iter()
                    .map(|point| point.to_array())
                    .collect::<Vec<_>>();
                let radii = vec![radius; positions.len()];
                push_tube(&mut vertices, &mut indices, &positions, &radii, object);
            }
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
