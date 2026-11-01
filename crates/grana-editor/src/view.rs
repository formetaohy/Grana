use crate::state::State;

pub fn fit_volume(state: &mut State<'_>) {
    let volume = state.stage.volume_rect();
    let section = state.stage.section_rect();
    state.viewport.fit(
        state.scene.dataset.world_size(),
        state.scene.dataset.voxel_size(),
        aspect(volume),
        aspect(section),
    );
}

pub fn fit_section(state: &mut State<'_>) {
    let section = state.stage.section_rect();
    state.viewport.fit_section(
        state.scene.dataset.world_size(),
        state.scene.dataset.voxel_size(),
        aspect(section),
    );
}

fn aspect(rect: grana_ui::Rect) -> f32 {
    rect.width() / rect.height().max(1.0)
}
