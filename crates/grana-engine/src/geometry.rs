use std::sync::Arc;

use glam::Vec3;
use grana_geometry::TriangleBudget;
use grana_object::SurfaceSet;
use grana_render::Digest;
use grana_scene::{ObjectSet, ObjectSetId, Scene};

use crate::view::Lens;

const PIXEL_TRIANGLE_AREA: f32 = 4.0;
const MINIMUM_TARGET: f32 = 512.0;

pub(crate) struct GeometryCache {
    entries: Vec<Entry>,
}

struct Entry {
    set: ObjectSetId,
    stamp: u64,
    points: Arc<[[f32; 4]]>,
}

impl GeometryCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn retain(&mut self, present: &[ObjectSetId]) {
        self.entries.retain(|entry| present.contains(&entry.set));
    }

    pub(crate) fn target(
        &mut self,
        scene: &Scene,
        set: &ObjectSet,
        surfaces: &SurfaceSet,
        lens: Lens<'_>,
    ) -> TriangleBudget {
        let stamp = stamp(scene, set);
        let index = match self.entries.iter().position(|entry| entry.set == set.id()) {
            Some(index) => index,
            None => {
                self.entries.push(Entry {
                    set: set.id(),
                    stamp: 0,
                    points: Arc::from(Vec::new()),
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        if entry.stamp != stamp {
            entry.points = Arc::from(
                surfaces
                    .objects()
                    .iter()
                    .map(|object| {
                        let bounds = object.bounds();
                        let center = bounds.center();
                        let radius = 0.5 * Vec3::from(bounds.size()).length();
                        [center[0], center[1], center[2], radius]
                    })
                    .collect::<Vec<_>>(),
            );
            entry.stamp = stamp;
        }
        triangle_area(&entry.points, lens)
    }
}

fn stamp(scene: &Scene, set: &ObjectSet) -> u64 {
    let mut stamp = Digest::new();
    stamp.mix(set.id().raw());
    stamp.mix(u64::from(scene.time()));
    stamp.mix(set.revision().raw());
    stamp.mix(
        set.frame_stamp(scene.time())
            .map_or(u64::MAX, |held| held.raw()),
    );
    stamp.finish()
}

fn triangle_area(points: &[[f32; 4]], lens: Lens<'_>) -> TriangleBudget {
    let focal = lens.camera.pixels_per_world(lens.height);
    let eye = lens.camera.eye();
    let standoff = lens.camera.distance();
    let orthographic = lens.camera.projection().orthographic();
    let mut area = 0.0f64;
    for point in points {
        let center = Vec3::new(point[0], point[1], point[2]);
        let radius = point[3];
        let distance = if orthographic {
            standoff
        } else {
            (eye - center).length().max(0.5 * radius)
        }
        .max(1.0e-6);
        let pixels = f64::from(radius * focal / distance);
        area += std::f64::consts::PI * pixels * pixels / f64::from(PIXEL_TRIANGLE_AREA);
    }
    TriangleBudget::new(area.max(f64::from(MINIMUM_TARGET)) as usize)
}
