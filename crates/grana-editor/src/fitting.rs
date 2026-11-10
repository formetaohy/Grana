use grana_display::Camera;
use grana_scene::Scene;

pub fn fit_camera(scene: &Scene, camera: &mut Camera, aspect: f32) {
    camera.fit(scene.dataset.world_size(), aspect);
}
