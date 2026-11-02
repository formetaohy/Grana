use glam::Vec3;
use grana_display::Ray;
use grana_geometry::{GeometryGroup, PackedMesh};
use grana_object::Spot;
use grana_volume::Axis;

use crate::scene::{Item, Scene, Selection};

const UNIT: f32 = 65535.0;
const EDGE_TOLERANCE: f32 = -1.0e-6;
const EDGE_PROBE: f32 = 1.0e-5;

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
    let mut spot: Option<(f32, Selection)> = None;
    let mut solid: Option<(f32, Selection)> = None;
    for set in scene.pickable() {
        if let Some(frame) = set.frame(time) {
            if let Some(spots) = frame.spots() {
                for (index, value) in spots.spots().iter().enumerate() {
                    if !set.visible_object(time, index) {
                        continue;
                    }
                    if let Some(depth) = sphere_section(value, axis, point) {
                        nearer(
                            &mut spot,
                            depth,
                            Selection {
                                set: set.id(),
                                item: Item::Object(index),
                            },
                        );
                    }
                }
            }
            if let Some(mesh) = frame.geometry().filter(|mesh| !mesh.is_empty())
                && let Some((reach, object)) =
                    cut_object(mesh, axis, point, |object| set.visible_object(time, object))
            {
                nearer(
                    &mut solid,
                    reach,
                    Selection {
                        set: set.id(),
                        item: Item::Object(object),
                    },
                );
            }
        }
        if let Some(tracks) = set.tracks().filter(|tracks| tracks.visible())
            && let Some((reach, track)) =
                cut_object(tracks.mesh(), axis, point, |track| set.visible_track(track))
        {
            nearer(
                &mut solid,
                reach,
                Selection {
                    set: set.id(),
                    item: Item::Track(track),
                },
            );
        }
    }
    spot.or(solid).map(|(_, selection)| selection)
}

fn nearer(best: &mut Option<(f32, Selection)>, reach: f32, selection: Selection) {
    if best.is_none_or(|(nearest, _)| reach < nearest) {
        *best = Some((reach, selection));
    }
}

fn cut_object(
    mesh: &PackedMesh,
    axis: Axis,
    point: Vec3,
    visible: impl Fn(usize) -> bool,
) -> Option<(f32, usize)> {
    let mut best: Option<(f32, usize)> = None;
    for object in 0..mesh.object_count() {
        if !visible(object) {
            continue;
        }
        let group = &mesh.groups()[object];
        if !crosses(group, axis, point) || !contains(mesh, object, axis, point) {
            continue;
        }
        let reach = reach(group, axis);
        if best.is_none_or(|(nearest, _)| reach < nearest) {
            best = Some((reach, object));
        }
    }
    best
}

fn crosses(group: &GeometryGroup, axis: Axis, point: Vec3) -> bool {
    let index = axis.index();
    let low = group.min()[index];
    let high = low + UNIT * group.scale()[index];
    (low..=high).contains(&point[index])
}

fn reach(group: &GeometryGroup, axis: Axis) -> f32 {
    let (u_axis, v_axis) = axis.in_plane();
    UNIT * group.scale()[u_axis.index()] * UNIT * group.scale()[v_axis.index()]
}

fn contains(mesh: &PackedMesh, object: usize, axis: Axis, point: Vec3) -> bool {
    let (u_axis, v_axis) = axis.in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let plane = axis.index();
    let group = &mesh.groups()[object];
    let mut probe = point;
    probe[u] += EDGE_PROBE * UNIT * group.scale()[u];
    let mut crossings = 0u32;
    for triangle in mesh_triangles(mesh, object) {
        let [first, second, third] = triangle.map(Vec3::from);
        let denominator = (second[u] - first[u]) * (third[v] - first[v])
            - (third[u] - first[u]) * (second[v] - first[v]);
        if denominator.abs() < 1.0e-12 {
            continue;
        }
        let offset_u = probe[u] - first[u];
        let offset_v = probe[v] - first[v];
        let across =
            (offset_u * (third[v] - first[v]) - offset_v * (third[u] - first[u])) / denominator;
        let down =
            (offset_v * (second[u] - first[u]) - offset_u * (second[v] - first[v])) / denominator;
        let up = 1.0 - across - down;
        if across < EDGE_TOLERANCE || down < EDGE_TOLERANCE || up < EDGE_TOLERANCE {
            continue;
        }
        let height = across * first[plane] + down * second[plane] + up * third[plane];
        if height > probe[plane] {
            crossings += 1;
        }
    }
    crossings % 2 == 1
}

fn mesh_triangles(mesh: &PackedMesh, object: usize) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
    let count = mesh.groups()[object].index_count() as usize / 3;
    (0..count).map(move |triangle| mesh.triangle(object, triangle))
}

fn sphere_section(spot: &Spot, axis: Axis, point: Vec3) -> Option<f32> {
    let (u_axis, v_axis) = axis.in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let plane = axis.index();
    let offset = Vec3::from(spot.center()) - point;
    let depth = offset[plane].abs();
    let radius = spot.radius();
    if depth > radius {
        return None;
    }
    let planar = Vec3::new(offset[u], offset[v], 0.0).length();
    if planar > (radius * radius - depth * depth).sqrt() {
        return None;
    }
    Some(depth)
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
