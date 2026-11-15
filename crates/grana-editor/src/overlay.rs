use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use grana_figure::Label;
use grana_scene::{Item, Scene};

#[derive(Default)]
pub struct Overlay {
    stamp: Option<u64>,
    labels: Vec<Label>,
}

impl Overlay {
    pub fn refresh(&mut self, scene: &Scene) {
        let stamp = stamp(scene);
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        self.labels = grana_figure::labels(scene, scene.time());
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }
}

fn stamp(scene: &Scene) -> u64 {
    let mut hasher = DefaultHasher::new();
    scene.time().hash(&mut hasher);
    let clip = scene.clip;
    clip.enabled().hash(&mut hasher);
    clip.normal().to_array().map(f32::to_bits).hash(&mut hasher);
    clip.offset().to_bits().hash(&mut hasher);
    for selection in scene.selections() {
        selection.set.raw().hash(&mut hasher);
        match selection.item {
            Item::Object(index) => {
                0u8.hash(&mut hasher);
                index.hash(&mut hasher);
            }
            Item::Track(index) => {
                1u8.hash(&mut hasher);
                index.hash(&mut hasher);
            }
        }
    }
    for set in scene.objects.iter() {
        set.id().raw().hash(&mut hasher);
        set.revision().raw().hash(&mut hasher);
        set.document().raw().hash(&mut hasher);
        set.frame_stamp(scene.time())
            .map_or(u64::MAX, |stamp| stamp.raw())
            .hash(&mut hasher);
        scene.shown(set).hash(&mut hasher);
    }
    hasher.finish()
}
