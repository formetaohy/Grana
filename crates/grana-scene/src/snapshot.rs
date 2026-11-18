use grana_raster::Overlay;

use crate::dataset::Dataset;
use crate::object_set::ObjectSetId;
use crate::scene::Scene;
use crate::state::SceneState;

pub struct Snapshot {
    dataset: Dataset,
    state: SceneState,
    overlays: Vec<(ObjectSetId, Overlay)>,
}

impl Scene {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            dataset: self
                .dataset
                .with_channels(self.dataset.channels()[..self.base_channels()].to_vec()),
            state: self.state(),
            overlays: self.overlays.snapshot(),
        }
    }
}

impl Snapshot {
    pub fn into_scene(self) -> Scene {
        let mut scene = Scene::new(self.dataset);
        scene.restore(self.state);
        for (set, overlay) in self.overlays {
            scene.overlays.set(set, overlay);
        }
        scene
    }
}
