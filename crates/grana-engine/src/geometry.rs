use glam::Vec3;
use grana_geometry::{Bounds, SphereTree, TriangleBudget};
use grana_object::SurfaceSet;
use grana_render::Digest;
use grana_scene::{ObjectSet, ObjectSetId, Scene};

use crate::view::Lens;

const PIXEL_TRIANGLE_AREA: f64 = 4.0;

const MINIMUM_TARGET: f64 = 512.0;

pub(crate) struct GeometryCache {
    entries: Vec<Entry>,
}

struct Entry {
    set: ObjectSetId,
    stamp: Option<u64>,
    tree: SphereTree,
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
                    stamp: None,
                    tree: SphereTree::new([]),
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        if entry.stamp != Some(stamp) {
            entry.tree = SphereTree::new(
                surfaces
                    .objects()
                    .iter()
                    .map(|object| sphere(object.bounds())),
            );
            entry.stamp = Some(stamp);
        }
        budget(&entry.tree, lens)
    }
}

fn sphere(bounds: Bounds) -> [f32; 4] {
    let center = bounds.center();
    [
        center[0],
        center[1],
        center[2],
        0.5 * Vec3::from(bounds.size()).length(),
    ]
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

fn budget(tree: &SphereTree, lens: Lens<'_>) -> TriangleBudget {
    let focal = f64::from(lens.camera.pixels_per_world(lens.height));
    let share = if lens.camera.projection().orthographic() {
        let standoff = f64::from(lens.camera.distance());
        tree.mass() / (standoff * standoff)
    } else {
        tree.subtense(lens.camera.eye())
    };
    let area = std::f64::consts::PI * focal * focal * share / PIXEL_TRIANGLE_AREA;
    TriangleBudget::new(area.max(MINIMUM_TARGET) as usize)
}
