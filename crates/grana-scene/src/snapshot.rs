use crate::dataset::Dataset;
use crate::scene::Scene;
use crate::state::SceneState;

pub struct Snapshot {
    dataset: Dataset,
    state: SceneState,
}

impl Scene {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            dataset: self
                .dataset
                .with_channels(self.dataset.channels()[..self.base_channels()].to_vec()),
            state: self.state(),
        }
    }
}

impl Snapshot {
    pub fn into_scene(self) -> Scene {
        let mut scene = Scene::new(self.dataset);
        scene.restore(self.state);
        scene
    }
}
