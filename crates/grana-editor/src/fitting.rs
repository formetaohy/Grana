use grana_display::Viewport;
use grana_scene::Scene;

use crate::stage::Stage;

pub fn fit_volume(scene: &Scene, viewport: &mut Viewport, stage: &Stage) {
    let volume = stage.volume_rect();
    let section = stage.section_rect();
    viewport.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        aspect(volume),
        aspect(section),
    );
}

pub fn fit_section(scene: &Scene, viewport: &mut Viewport, stage: &Stage) {
    let section = stage.section_rect();
    viewport.fit_section(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        aspect(section),
    );
}

fn aspect(rect: grana_ui::Rect) -> f32 {
    rect.width() / rect.height().max(1.0)
}
