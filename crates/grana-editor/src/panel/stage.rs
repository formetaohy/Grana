use glam::Vec2;
use grana_figure::{self as overlay, Overlay};
use grana_ui::glass::GlassStyle;
use grana_ui::{Color, Corners, Cursor, Icon, Level, Motion, Rect, TextStyle, Ui, icon};

use crate::state::State;
use grana_session::Command;

use super::{Layout, scene_uv};

const PANE_CONTROL: f32 = 26.0;

pub(super) fn paint(
    ui: &mut Ui,
    layout: &Layout,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let size = ui.size();
    let palette = ui.theme().palette;
    ui.content(|ui| {
        let mut painter = ui.content_painter();
        painter.rect(layout.stage, Corners::ZERO, palette.scene_backdrop);
        for pane in [layout.volume, layout.section] {
            painter.scene(pane, Corners::ZERO, scene_uv(pane, size));
        }
    });
    paint_overlay(ui, layout, state, commands);
    pane_fit(
        ui,
        layout.volume,
        layout.toolbar_left.max.y,
        "volume.fit",
        "Fit volume",
        crate::view::fit_volume,
        state,
    );
    pane_fit(
        ui,
        layout.section,
        layout.toolbar_left.max.y,
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
    top: f32,
    id: &str,
    tooltip: &str,
    action: fn(&mut State<'_>),
    state: &mut State<'_>,
) {
    let rect = Rect::from_min_size(
        Vec2::new(pane.max.x - PANE_CONTROL - 12.0, top + 12.0),
        Vec2::splat(PANE_CONTROL),
    );
    ui.place(rect, |ui| {
        if grana_ui::widget::glass_icon_button(
            ui,
            None,
            id,
            Icon::Fit,
            PANE_CONTROL,
            GlassStyle::stage_control(),
            tooltip,
        ) {
            action(state);
        }
    });
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
    let labels = overlay::labels(state.scene, state.scene.time());
    let safe_left = if layout.sidebar_visible {
        layout.sidebar.max.x.max(layout.stage.min.x) + 18.0
    } else {
        layout.stage.min.x + 18.0
    };
    let safe_top = layout.toolbar_left.max.y + 18.0;
    let safe_bottom = layout
        .dock
        .or(layout.transport)
        .map_or(layout.stage.max.y - 18.0, |rect| rect.min.y - 12.0);
    let safe = Rect::from_min_max(
        Vec2::new(safe_left, safe_top),
        Vec2::new(layout.volume.max.x - 18.0, safe_bottom),
    );
    let section_safe = Rect::from_min_max(
        Vec2::new(layout.section.min.x + 12.0, safe_top),
        Vec2::new(layout.section.max.x, safe_bottom),
    );
    let mut overlay = Overlay {
        camera: state.viewport.camera(),
        section: state.viewport.section(),
        world,
        volume_safe: safe,
        section_safe,
        section_stage: stage,
        volume_rect: layout.volume,
        section_rect: layout.section,
        region: &mut region,
        clip,
        legends: &legends,
        labels: &labels,
        recording: state.io.export,
    };
    overlay::paint(ui, &mut overlay);
    if region != state.scene.region {
        commands.push(Command::SetRegion(region));
    }
}

fn split_handle(ui: &mut Ui, volume: Rect, section: Rect, viewport: &mut grana_display::Viewport) {
    let x = (volume.max.x + section.min.x) * 0.5;
    let grab = Rect::from_center_size(
        Vec2::new(x, volume.center().y),
        Vec2::new(18.0, volume.height()),
    );
    let response = ui.interact(ui.id("split.handle"), grab);
    ui.register_occluder(grab);
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.dragging {
        let pointer = ui.input().position().x;
        let total = (volume.width() + section.width()).max(1.0);
        viewport.set_split(((pointer - volume.min.x) / total).clamp(0.3, 0.75));
    }
    let engaged = ui.animate(
        ui.id("split.grow"),
        if response.hovered || response.dragging {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let palette = ui.theme().palette;
    let size = Vec2::new(8.0 + engaged * 5.0, 36.0 + engaged * 20.0);
    let rect = Rect::from_center_size(Vec2::new(x, volume.center().y), size);
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_center_size(
                Vec2::new(x, volume.center().y),
                Vec2::new(1.0, volume.height()),
            ),
            Corners::ZERO,
            palette.separator.amplify(0.6),
        );
    }
    let lens_key = ui.id("split.lens");
    ui.glass(lens_key, rect, GlassStyle::lens());
    ui.engage_glass(lens_key, response.active);
    let mut painter = ui.labels_painter();
    painter.rect(
        Rect::from_center_size(rect.center(), Vec2::new(1.4, size.y * 0.34)),
        Corners::capsule(),
        palette.text_primary.with_alpha(0.26 + engaged * 0.44),
    );
    if engaged > 0.01 {
        painter.rect(
            Rect::from_center_size(
                Vec2::new(x, volume.center().y),
                Vec2::new(1.0, 26.0 + 10.0 * engaged),
            ),
            Corners::capsule(),
            Color::white_alpha(0.12 * engaged),
        );
    }
}

pub(super) fn toast(ui: &mut Ui, layout: &Layout, state: &State<'_>) {
    let Some(message) = state.io.message else {
        return;
    };
    let palette = ui.theme().palette;
    let stage = layout.stage;
    let ceiling = layout
        .dock
        .or(layout.transport)
        .map_or(stage.max.y - 28.0, |rect| rect.min.y - 16.0);
    let width = (stage.width() - 28.0).min(440.0);
    let style = TextStyle::new(ui.theme().font(Level::Callout), palette.text_primary).wrapping();
    let layout_text = ui.layout_text(message.text(), style, width - 60.0);
    let height = (layout_text.size.y + 24.0).max(46.0);
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, ceiling - height),
        Vec2::new(width, height),
    );
    let (mark, tint) = match message.kind() {
        grana_session::Kind::Error => (Icon::Warning, palette.warning),
        grana_session::Kind::Info => (Icon::Info, palette.text_secondary),
    };
    let appear = ui.appeared(ui.id(message.text()), 0.25);
    ui.glass(ui.id("toast.glass"), rect, GlassStyle::toast());
    ui.hud_scope(rect, |ui| {
        let mut painter = ui.painter();
        icon::paint(
            mark,
            &mut painter,
            Vec2::new(rect.min.x + 22.0, rect.center().y),
            16.0,
            tint.with_alpha(tint.alpha * appear),
            1.6,
        );
        painter.text(
            &layout_text,
            Vec2::new(rect.min.x + 40.0, rect.min.y + 12.0),
            palette
                .text_primary
                .with_alpha(palette.text_primary.alpha * appear),
        );
    });
}
