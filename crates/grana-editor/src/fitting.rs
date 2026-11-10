use grana_display::Panes;
use grana_scene::Scene;

pub fn fit_pane(scene: &Scene, panes: &mut Panes, index: usize, aspect: f32) {
    panes.fit_pane(
        index,
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        aspect,
    );
}
