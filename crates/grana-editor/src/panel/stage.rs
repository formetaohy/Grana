use glam::Vec2;
use grana_figure::{self as overlay, Overlay};
use grana_ui::{Corners, Cursor, Icon, Level, Rect, Role, TextStyle, Ui, icon};

use crate::state::State;
use grana_session::Command;

use super::{Layout, SPLITTER, scene_uv};

pub(super) fn paint(
    ui: &mut Ui,
    layout: &Layout,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let size = ui.size();
    let palette = ui.theme().palette;
    {
        let mut painter = ui.painter();
        painter.rect(layout.stage, Corners::ZERO, palette.scene_backdrop);
        painter.scene(layout.volume, Corners::ZERO, scene_uv(layout.volume, size));
        painter.scene(
            layout.section,
            Corners::ZERO,
            scene_uv(layout.section, size),
        );
        painter.rect(
            Rect::from_min_size(
                Vec2::new(layout.volume.max.x, layout.stage.min.y),
                Vec2::new(SPLITTER, layout.stage.height()),
            ),
            Corners::ZERO,
            palette.border,
        );
    }
    paint_overlay(ui, layout, state, commands);
    pane_fit(
        ui,
        layout.volume,
        "volume.fit",
        "Fit volume",
        crate::view::fit_volume,
        state,
    );
    pane_fit(
        ui,
        layout.section,
        "section.fit",
        "Fit section",
        crate::view::fit_section,
        state,
    );
    split_handle(ui, layout.volume, layout.section, state.viewport);
}

fn pane_fit(
    ui: &mut Ui,
    pane: Rect,
    id: &str,
    tooltip: &str,
    action: fn(&mut State<'_>),
    state: &mut State<'_>,
) {
    let rect = Rect::from_min_size(
        Vec2::new(pane.max.x - 12.0 - 28.0, pane.min.y + 12.0),
        Vec2::splat(28.0),
    );
    ui.place(rect, |ui| {
        if grana_ui::widget::icon_button(ui, id, Icon::Fit, 28.0, Role::Hud, tooltip) {
            action(state);
        }
    });
    ui.register_occluder(rect);
}

fn paint_overlay(ui: &mut Ui, layout: &Layout, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let axis = state.viewport.section().axis();
    let (plane_u, plane_v) = axis.in_plane();
    let world = state.scene.dataset.world_size();
    let points_per_micron = layout.section.height() / state.viewport.section().span();
    let stage = Rect::from_center_size(
        layout.section.center(),
        Vec2::new(
            world[plane_u.index()] * points_per_micron,
            world[plane_v.index()] * points_per_micron,
        ),
    )
    .intersect(layout.section);
    let mut region = state.scene.region;
    let clip = state.scene.clip;
    let legends = overlay::legends(state.scene);
    let mut overlay = Overlay {
        camera: state.viewport.camera(),
        section: state.viewport.section(),
        world,
        volume_safe: layout.volume,
        section_stage: stage,
        volume_rect: layout.volume,
        section_rect: layout.section,
        region: &mut region,
        clip,
        legends: &legends,
        recording: state.io.export,
    };
    overlay::paint(ui, &mut overlay);
    if region != state.scene.region {
        commands.push(Command::SetRegion(region));
    }
}

fn split_handle(ui: &mut Ui, volume: Rect, section: Rect, viewport: &mut grana_display::Viewport) {
    let x = volume.max.x + SPLITTER * 0.5;
    let rect = Rect::from_center_size(
        Vec2::new(x, volume.center().y),
        Vec2::new(7.0, volume.height()),
    );
    let response = ui.interact(ui.id("split.handle"), rect);
    ui.register_occluder(rect);
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.dragging {
        let pointer = ui.input().position().x;
        let total = (volume.width() + section.width() + SPLITTER).max(1.0);
        viewport.set_split(((pointer - volume.min.x) / total).clamp(0.3, 0.75));
    }
    if response.hovered || response.dragging {
        let palette = ui.theme().palette;
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(
                Vec2::new(x - 0.5, volume.min.y),
                Vec2::new(1.0, volume.height()),
            ),
            Corners::ZERO,
            palette.text_quaternary,
        );
    }
}

pub(super) fn toast(ui: &mut Ui, stage: Rect, state: &State<'_>) {
    let Some(message) = state.io.message else {
        return;
    };
    let palette = ui.theme().palette;
    let width = (stage.width() - 28.0).min(360.0);
    let style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_primary).wrapping();
    let layout = ui.layout_text(message.text(), style, width - 44.0);
    let height = (layout.size.y + 20.0).max(38.0);
    let rect = Rect::from_min_size(
        Vec2::new(stage.max.x - 14.0 - width, stage.max.y - 14.0 - height),
        Vec2::new(width, height),
    );
    let look = ui.theme().look(Role::Hud, grana_ui::State::Rest);
    let (mark, tint) = match message.kind() {
        grana_session::Kind::Error => (Icon::Warning, palette.warning),
        grana_session::Kind::Info => (Icon::Info, palette.text_secondary),
    };
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    icon::paint(
        mark,
        &mut painter,
        Vec2::new(rect.min.x + 16.0, rect.center().y),
        15.0,
        tint,
        1.5,
    );
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x, rect.min.y + 8.0),
            Vec2::new(2.0, rect.height() - 16.0),
        ),
        Corners::capsule(),
        tint.with_alpha(0.6),
    );
    painter.text(
        &layout,
        Vec2::new(rect.min.x + 32.0, rect.min.y + 10.0),
        palette.text_primary,
    );
}
