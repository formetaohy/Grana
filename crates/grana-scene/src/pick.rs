use glam::Vec3;
use grana_display::Ray;
use grana_geometry::PackedMesh;
use grana_object::Spot;
use grana_volume::Axis;

use crate::scene::{Item, Scene, Selection};

pub(crate) fn ray(scene: &Scene, time: u32, ray: &Ray) -> Option<Selection> {
    let (low, high) = scene.region.interval(ray.origin, ray.direction)?;
    let mut best: Option<(f32, Selection)> = None;
    for set in scene.pickable() {
        if let Some(frame) = set.frame(time) {
            if let Some(spots) = frame.spots() {
                for (index, spot) in spots.spots().iter().enumerate() {
                    if !set.visible_object(time, index) {
                        continue;
                    }
                    let Some((near, far)) = ray_sphere(ray, *spot) else {
                        continue;
                    };
                    if far < low || near > high {
                        continue;
                    }
                    let distance = near.max(low);
                    let selection = Selection {
                        set: set.id(),
                        item: Item::Object(index),
                    };
                    if best.is_none_or(|(nearest, _)| distance < nearest) {
                        best = Some((distance, selection));
                    }
                }
            }
            if let Some(mesh) = frame.geometry().filter(|mesh| !mesh.is_empty()) {
                for object in 0..mesh.object_count() {
                    if !set.visible_object(time, object) {
                        continue;
                    }
                    for distance in mesh_triangles(mesh, object).filter_map(|triangle| {
                        ray_triangle(ray, triangle)
                            .filter(|distance| *distance >= low && *distance <= high)
                    }) {
                        let selection = Selection {
                            set: set.id(),
                            item: Item::Object(object),
                        };
                        if best.is_none_or(|(nearest, _)| distance < nearest) {
                            best = Some((distance, selection));
                        }
                    }
                }
            }
        }
        if let Some(tracks) = set.tracks().filter(|tracks| tracks.visible()) {
            let mesh = tracks.mesh();
            for track in 0..mesh.object_count() {
                if !set.visible_track(track) {
                    continue;
                }
                for distance in mesh_triangles(mesh, track).filter_map(|triangle| {
                    ray_triangle(ray, triangle)
                        .filter(|distance| *distance >= low && *distance <= high)
                }) {
                    let selection = Selection {
                        set: set.id(),
                        item: Item::Track(track),
                    };
                    if best.is_none_or(|(nearest, _)| distance < nearest) {
                        best = Some((distance, selection));
                    }
                }
            }
        }
    }
    best.map(|(_, selection)| selection)
}

pub(crate) fn slice(scene: &Scene, time: u32, axis: Axis, point: Vec3) -> Option<Selection> {
    if !scene.region.contains(point) {
        return None;
    }
    let mut best: Option<(f32, Selection)> = None;
    for set in scene.pickable() {
        if let Some(frame) = set.frame(time) {
            if let Some(spots) = frame.spots() {
                let plane = axis.index();
                let (u_axis, v_axis) = axis.in_plane();
                let (u, v) = (u_axis.index(), v_axis.index());
                for (index, spot) in spots.spots().iter().enumerate() {
                    if !set.visible_object(time, index) {
                        continue;
                    }
                    let offset = Vec3::from(spot.center()) - point;
                    let depth = offset[plane].abs();
                    let radius = spot.radius();
                    if depth > radius {
                        continue;
                    }
                    let planar = Vec3::new(offset[u], offset[v], 0.0).length();
                    if planar > (radius * radius - depth * depth).sqrt() {
                        continue;
                    }
                    let selection = Selection {
                        set: set.id(),
                        item: Item::Object(index),
                    };
                    if best.is_none_or(|(nearest, _)| depth < nearest) {
                        best = Some((depth, selection));
                    }
                }
            }
            if let Some(mesh) = frame.geometry().filter(|mesh| !mesh.is_empty()) {
                for object in 0..mesh.object_count() {
                    if !set.visible_object(time, object) {
                        continue;
                    }
                    if let Some(offset) = slice_mesh(mesh, object, axis, point)
                        && best.is_none_or(|(nearest, _)| offset < nearest)
                    {
                        best = Some((
                            offset,
                            Selection {
                                set: set.id(),
                                item: Item::Object(object),
                            },
                        ));
                    }
                }
            }
        }
        if let Some(tracks) = set.tracks().filter(|tracks| tracks.visible()) {
            let mesh = tracks.mesh();
            for track in 0..mesh.object_count() {
                if !set.visible_track(track) {
                    continue;
                }
                if let Some(offset) = slice_mesh(mesh, track, axis, point)
                    && best.is_none_or(|(nearest, _)| offset < nearest)
                {
                    best = Some((
                        offset,
                        Selection {
                            set: set.id(),
                            item: Item::Track(track),
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, selection)| selection)
}

fn mesh_triangles(mesh: &PackedMesh, object: usize) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
    let count = mesh.groups()[object].index_count() as usize / 3;
    (0..count).map(move |triangle| mesh.triangle(object, triangle))
}

fn slice_mesh(mesh: &PackedMesh, object: usize, axis: Axis, point: Vec3) -> Option<f32> {
    let plane = axis.index();
    let (u_axis, v_axis) = axis.in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let mut nearest: Option<(f32, bool)> = None;
    for triangle in mesh_triangles(mesh, object) {
        let Some((offset, exit)) = slice_triangle(triangle, plane, u, v, point, axis) else {
            continue;
        };
        if offset < 0.0 {
            continue;
        }
        if nearest.is_none_or(|(nearest_offset, _)| offset < nearest_offset) {
            nearest = Some((offset, exit));
        }
    }
    match nearest {
        Some((offset, true)) => Some(offset),
        _ => None,
    }
}

fn ray_sphere(ray: &Ray, spot: Spot) -> Option<(f32, f32)> {
    let offset = Vec3::from(spot.center()) - ray.origin;
    let projection = offset.dot(ray.direction);
    let radius = spot.radius();
    let squared = offset.length_squared() - projection * projection;
    let radius_squared = radius * radius;
    if squared > radius_squared {
        return None;
    }
    let half = (radius_squared - squared).sqrt();
    if projection + half < 0.0 {
        return None;
    }
    Some((projection - half, projection + half))
}

fn ray_triangle(ray: &Ray, triangle: [[f32; 3]; 3]) -> Option<f32> {
    let [first, second, third] = triangle.map(Vec3::from);
    let edge = second - first;
    let across = third - first;
    let h = ray.direction.cross(across);
    let determinant = edge.dot(h);
    if determinant.abs() < 1.0e-12 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let offset = ray.origin - first;
    let u = inverse * offset.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = offset.cross(edge);
    let v = inverse * ray.direction.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = inverse * across.dot(q);
    (distance > 1.0e-6).then_some(distance)
}

fn slice_triangle(
    triangle: [[f32; 3]; 3],
    plane: usize,
    u: usize,
    v: usize,
    point: Vec3,
    axis: Axis,
) -> Option<(f32, bool)> {
    let positions = triangle.map(Vec3::from);
    let (a, b, c) = (positions[0], positions[1], positions[2]);
    let denominator = (b[u] - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (b[v] - a[v]);
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    let du = point[u] - a[u];
    let dv = point[v] - a[v];
    let first = (du * (c[v] - a[v]) - dv * (c[u] - a[u])) / denominator;
    let second = (dv * (b[u] - a[u]) - du * (b[v] - a[v])) / denominator;
    let third = 1.0 - first - second;
    if first < -1.0e-6 || second < -1.0e-6 || third < -1.0e-6 {
        return None;
    }
    let offset = first * (a[plane] - point[plane])
        + second * (b[plane] - point[plane])
        + third * (c[plane] - point[plane]);
    let mut normal = (b - a).cross(c - a);
    if normal.length_squared() < 1.0e-20 {
        return None;
    }
    normal = normal.normalize();
    let mut direction = Vec3::ZERO;
    direction[axis.index()] = 1.0;
    Some((offset, normal.dot(direction) > 0.0))
}
