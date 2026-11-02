use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use glam::Vec3;
use grana_display::Ray;
use grana_geometry::{Bvh, PackedMesh, Primitive};
use grana_object::Spot;
use grana_volume::Axis;

use crate::scene::{Item, Scene, Selection};

const UNIT: f32 = 65535.0;

#[derive(Clone, Copy)]
enum Target {
    Spot(Selection),
    Solid {
        selection: Selection,
        low: [f32; 3],
        high: [f32; 3],
    },
}

impl Target {
    fn selection(self) -> Selection {
        match self {
            Self::Spot(selection) | Self::Solid { selection, .. } => selection,
        }
    }
}

#[derive(Default)]
pub(crate) struct Cache {
    key: Option<u64>,
    bvh: Option<Bvh>,
    targets: Vec<Target>,
}

impl Cache {
    fn sync(&mut self, scene: &Scene, time: u32) {
        let key = digest(scene, time);
        if self.key == Some(key) {
            return;
        }
        let (bvh, targets) = build(scene, time);
        self.key = Some(key);
        self.bvh = Some(bvh);
        self.targets = targets;
    }
}

pub(crate) fn ray(scene: &Scene, time: u32, ray: &Ray) -> Option<Selection> {
    let (low, high) = scene.region.interval(ray.origin, ray.direction)?;
    let mut cache = scene.pick.borrow_mut();
    cache.sync(scene, time);
    let bvh = cache.bvh.as_ref()?;
    let origin = ray.origin + ray.direction * low;
    let (_, index) = bvh.ray(origin, ray.direction, high - low)?;
    Some(cache.targets[index as usize].selection())
}

pub(crate) fn slice(scene: &Scene, time: u32, axis: Axis, point: Vec3) -> Option<Selection> {
    if !scene.region.contains(point) {
        return None;
    }
    let mut spot: Option<(f32, Selection)> = None;
    for set in scene.pickable() {
        let Some(spots) = set.frame(time).and_then(|frame| frame.spots()) else {
            continue;
        };
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
    let mut solid: Option<(f32, Selection)> = None;
    let mut cache = scene.pick.borrow_mut();
    cache.sync(scene, time);
    let bvh = cache.bvh.as_ref()?;
    let mut containing = bvh.containing(point);
    containing.sort_unstable();
    for index in containing {
        let Target::Solid {
            selection,
            low,
            high,
        } = cache.targets[index as usize]
        else {
            continue;
        };
        if !crosses(low, high, axis, point) {
            continue;
        }
        nearer(&mut solid, reach(low, high, axis), selection);
    }
    spot.or(solid).map(|(_, selection)| selection)
}

fn build(scene: &Scene, time: u32) -> (Bvh, Vec<Target>) {
    let mut primitives = Vec::new();
    let mut targets = Vec::new();
    for set in scene.pickable() {
        if let Some(frame) = set.frame(time) {
            if let Some(spots) = frame.spots() {
                for (index, value) in spots.spots().iter().enumerate() {
                    if !set.visible_object(time, index) {
                        continue;
                    }
                    let id = targets.len() as u32;
                    targets.push(Target::Spot(Selection {
                        set: set.id(),
                        item: Item::Object(index),
                    }));
                    primitives.push((
                        id,
                        Primitive::sphere(Vec3::from(value.center()), value.radius()),
                    ));
                }
            }
            if let Some(mesh) = frame.geometry().filter(|mesh| !mesh.is_empty()) {
                for object in 0..mesh.object_count() {
                    if !set.visible_object(time, object) {
                        continue;
                    }
                    push_solid(
                        &mut primitives,
                        &mut targets,
                        mesh,
                        object,
                        Selection {
                            set: set.id(),
                            item: Item::Object(object),
                        },
                    );
                }
            }
        }
        if let Some(tracks) = set.tracks().filter(|tracks| tracks.visible()) {
            let mesh = tracks.mesh();
            for track in 0..mesh.object_count() {
                if !set.visible_track(track) {
                    continue;
                }
                push_solid(
                    &mut primitives,
                    &mut targets,
                    mesh,
                    track,
                    Selection {
                        set: set.id(),
                        item: Item::Track(track),
                    },
                );
            }
        }
    }
    (Bvh::new(primitives), targets)
}

fn push_solid(
    primitives: &mut Vec<(u32, Primitive)>,
    targets: &mut Vec<Target>,
    mesh: &PackedMesh,
    object: usize,
    selection: Selection,
) {
    let group = &mesh.groups()[object];
    let low = group.min();
    let high = [0, 1, 2].map(|axis| low[axis] + UNIT * group.scale()[axis]);
    let id = targets.len() as u32;
    targets.push(Target::Solid {
        selection,
        low,
        high,
    });
    for triangle in 0..group.index_count() as usize / 3 {
        primitives.push((
            id,
            Primitive::triangle(mesh.triangle(object, triangle).map(Vec3::from)),
        ));
    }
}

fn digest(scene: &Scene, time: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    time.hash(&mut hasher);
    for set in scene.pickable() {
        set.id().hash(&mut hasher);
        set.frame_stamp(time).hash(&mut hasher);
        if let Some(frame) = set.frame(time) {
            frame.object_count().hash(&mut hasher);
            for object in 0..frame.object_count() {
                set.visible_object(time, object).hash(&mut hasher);
            }
        }
        if let Some(tracks) = set.tracks() {
            tracks.visible().hash(&mut hasher);
            tracks.revision().hash(&mut hasher);
            tracks.set().len().hash(&mut hasher);
            for track in 0..tracks.set().len() {
                set.visible_track(track).hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn nearer(best: &mut Option<(f32, Selection)>, reach: f32, selection: Selection) {
    if best.is_none_or(|(nearest, _)| reach < nearest) {
        *best = Some((reach, selection));
    }
}

fn crosses(low: [f32; 3], high: [f32; 3], axis: Axis, point: Vec3) -> bool {
    let index = axis.index();
    (low[index]..=high[index]).contains(&point[index])
}

fn reach(low: [f32; 3], high: [f32; 3], axis: Axis) -> f32 {
    let (u_axis, v_axis) = axis.in_plane();
    (high[u_axis.index()] - low[u_axis.index()]) * (high[v_axis.index()] - low[v_axis.index()])
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
