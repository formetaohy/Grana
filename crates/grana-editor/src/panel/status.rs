use glam::Vec2;
use grana_ui::glass::GlassStyle;
use grana_ui::{Align, Corners, Rect, TextStyle, TypeStyle, Ui};

use crate::state::State;

pub(super) fn bar(ui: &mut Ui, rect: Rect, state: &State<'_>) {
    let palette = ui.theme().palette;
    let font = TypeStyle::new(10.5, 400.0, 14.0);
    let style = TextStyle::new(font, palette.text_tertiary);
    let inner = Rect::from_min_max(
        Vec2::new(rect.min.x + 18.0, rect.min.y),
        Vec2::new(rect.max.x - 18.0, rect.max.y),
    );
    let mut painter = ui.painter();
    let fade = Rect::from_min_max(
        Vec2::new(rect.min.x, rect.min.y - 22.0),
        Vec2::new(rect.max.x, rect.min.y),
    );
    painter.gradient(
        fade,
        Corners::ZERO,
        palette.scene_backdrop.with_alpha(0.0),
        palette.scene_backdrop.with_alpha(0.78),
    );
    painter.gradient(
        rect,
        Corners {
            top_left: 0.0,
            top_right: 0.0,
            bottom_right: 26.0,
            bottom_left: 26.0,
        },
        palette.scene_backdrop.with_alpha(0.78),
        palette.scene_backdrop.with_alpha(0.94),
    );
    painter.rect(
        Rect::from_min_size(
            Vec2::new(inner.min.x, inner.min.y + 2.0),
            Vec2::new(inner.width(), 1.0),
        ),
        Corners::ZERO,
        palette.separator,
    );
    painter.rect(
        Rect::from_center_size(
            Vec2::new(inner.min.x + 3.0, inner.min.y + 18.0),
            Vec2::splat(5.0),
        ),
        Corners::capsule(),
        palette.success,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(inner.min.x + 12.0, inner.min.y + 8.0),
            Vec2::new(inner.max.x, inner.min.y + 26.0),
        ),
        state.io.adapter,
        TextStyle::new(font, palette.text_secondary).truncating(),
        Align::Center,
    );
    let stats = format!(
        "level {}/{} · {} samples · {}× MSAA · {} OIT",
        state.frame.volume.level + 1,
        state.scene.dataset.level_count(),
        state.frame.samples,
        state.frame.msaa,
        state.frame.oit_layers
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(inner.min.x, inner.min.y + 27.0),
            Vec2::new(inner.max.x, rect.max.y - 8.0),
        ),
        &stats,
        style,
        Align::Center,
    );
}

pub(super) fn pill(ui: &mut Ui, anchor: Rect, state: &State<'_>) {
    let palette = ui.theme().palette;
    let rect = Rect::from_min_size(
        Vec2::new(anchor.min.x, anchor.max.y + 12.0),
        Vec2::new(340.0, 50.0),
    );
    ui.glass(ui.id("status.pill"), rect, GlassStyle::status());
    let font = TypeStyle::new(10.5, 400.0, 14.0);
    let inner = Rect::from_min_max(
        Vec2::new(rect.min.x + 16.0, rect.min.y + 7.0),
        Vec2::new(rect.max.x - 16.0, rect.max.y - 7.0),
    );
    let name = format!(
        "{} · {} frames",
        state.scene.dataset.name(),
        state.scene.frame_count()
    );
    let stats = format!(
        "level {}/{} · {} samples · {}× MSAA",
        state.frame.volume.level + 1,
        state.scene.dataset.level_count(),
        state.frame.samples,
        state.frame.msaa
    );
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_max(inner.min, Vec2::new(inner.max.x, inner.center().y)),
        &name,
        TextStyle::new(font, palette.text_secondary).truncating(),
        Align::Center,
    );
    painter.text_in(
        Rect::from_min_max(Vec2::new(inner.min.x, inner.center().y), inner.max),
        &stats,
        TextStyle::new(font, palette.text_quaternary),
        Align::Center,
    );
}
