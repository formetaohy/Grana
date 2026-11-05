use glam::Vec2;
use grana_ui::glass::{CHROME, GlassStyle};
use grana_ui::{Align, Corners, Rect, TextStyle, Ui};

use crate::state::State;

pub(super) fn bar(ui: &mut Ui, rect: Rect, state: &State<'_>) {
    let palette = ui.theme().palette;
    let font = grana_ui::TypeStyle::new(10.5, 400.0, 14.0);
    let style = TextStyle::new(font, palette.text_tertiary);
    let inner = Rect::from_min_max(
        Vec2::new(rect.min.x + 16.0, rect.min.y),
        Vec2::new(rect.max.x - 16.0, rect.max.y),
    );
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(inner.min.x, inner.min.y + 6.0),
            Vec2::new(inner.width(), 1.0),
        ),
        Corners::ZERO,
        palette.separator,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(inner.min.x, inner.min.y + 12.0),
            Vec2::new(inner.max.x, inner.min.y + 28.0),
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
            Vec2::new(inner.min.x, inner.min.y + 28.0),
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
        Vec2::new(320.0, 44.0),
    );
    ui.glass(
        ui.id("status.pill"),
        rect,
        GlassStyle::bar()
            .layer(CHROME)
            .with_shape(grana_ui::glass::GlassShape::Capsule),
    );
    let font = grana_ui::TypeStyle::new(10.5, 400.0, 14.0);
    let inner = Rect::from_min_max(
        Vec2::new(rect.min.x + 16.0, rect.min.y + 6.0),
        Vec2::new(rect.max.x - 16.0, rect.max.y - 6.0),
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
        state.io.adapter,
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
