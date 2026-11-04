use glam::Vec2;
use grana_ui::{Align, Rect, TextStyle, Ui};

use crate::state::State;

pub(super) fn bar(ui: &mut Ui, rect: Rect, state: &State<'_>) {
    let palette = ui.theme().palette;
    let font = grana_ui::TypeStyle::new(10.5, 400.0, 13.0);
    let style = TextStyle::new(font, palette.text_tertiary);
    let right = format!(
        "level {}/{} · {} samples · {:.0} MiB",
        state.frame.volume.level + 1,
        state.scene.dataset.level_count(),
        state.frame.samples,
        state.frame.bytes as f64 / (1024.0 * 1024.0)
    );
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(rect.min.x + 14.0, rect.min.y),
            Vec2::new(rect.max.x - 14.0, rect.max.y),
        ),
        state.io.adapter,
        style,
        Align::Center,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(rect.min.x + 14.0, rect.min.y),
            Vec2::new(rect.max.x - 14.0, rect.max.y),
        ),
        &right,
        TextStyle::new(font, palette.text_tertiary).aligned(Align::End),
        Align::Center,
    );
}
