use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, TypeStyle};
use crate::ui::Ui;

pub fn badge(ui: &mut Ui, label: &str, tint: Color) -> Rect {
    let font = TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.4,
        leading: 13.0,
    };
    let style = TextStyle::new(font, tint.with_alpha(0.92));
    let measured = ui.measure_text(label, style);
    let rect = ui.allocate(Vec2::new(measured.x + 14.0, 19.0));
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::capsule(),
        Fill::Solid(palette.material_fill.amplify(0.7)),
        Border {
            width: 1.0,
            color: palette.material_border,
        },
        Shadow::NONE,
    );
    painter.text_in(rect, label, style, Align::Center);
    rect
}

pub fn labelled(ui: &mut Ui, label: &str, value: &str) {
    let palette = ui.theme().palette;
    let name = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary);
    let content = TextStyle::new(ui.theme().font(Level::Monospace), palette.text_secondary);
    let width = ui.available().x;
    let label_width = (width * 0.38).clamp(64.0, 128.0);
    let rect = ui.allocate(Vec2::new(width, 22.0));
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_size(rect.min, Vec2::new(label_width, rect.height())),
        label,
        name.truncating(),
        Align::Center,
    );
    painter.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + label_width, rect.min.y),
            Vec2::new(rect.width() - label_width, rect.height()),
        ),
        value,
        TextStyle::new(content.font, content.color)
            .aligned(Align::End)
            .truncating(),
        Align::Center,
    );
}

pub fn section_title(ui: &mut Ui, title: &str) {
    let font = TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.8,
        leading: 14.0,
    };
    let style = TextStyle::new(font, ui.theme().palette.text_tertiary);
    let rect = ui.allocate_full(24.0);
    let mut painter = ui.painter();
    painter.text_in(rect, &title.to_uppercase(), style, Align::Center);
}

pub fn caption(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary);
    ui.text_style(text, style);
}

pub fn weak(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary);
    let available = ui.available().x;
    ui.text_style_wrapped(text, style.wrapping(), available);
}

pub fn body(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Body), palette.text_secondary);
    ui.text_style(text, style);
}

pub fn body_wrapped(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Body), palette.text_secondary);
    let available = ui.available().x;
    ui.text_style_wrapped(text, style.wrapping(), available);
}

pub fn headline(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Headline), palette.text_primary);
    ui.text_style(text, style);
}

pub fn title(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Title), palette.text_primary);
    ui.text_style(text, style);
}

pub fn field_label(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(ui.theme().label_font(), ui.theme().palette.text_tertiary);
    ui.text_style(text, style);
}

pub fn spinner(ui: &mut Ui, size: f32) {
    let rect = ui.allocate(Vec2::splat(size));
    let time = ui.time();
    let palette = ui.theme().palette;
    let center = rect.center();
    let mut painter = ui.painter();
    painter.ring(
        center,
        size * 0.38,
        1.6,
        0.0,
        std::f32::consts::TAU,
        palette.track.amplify(0.5),
    );
    painter.ring(center, size * 0.38, 1.6, time * 5.0, 1.7, palette.accent);
}

pub fn color_swatch(ui: &mut Ui, id: &str, color: [f32; 3], size: f32) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let response = ui.interact(key, rect);
    let hover = ui.animate(
        key ^ 0x61,
        if response.hovered { 1.0 } else { 0.0 },
        crate::motion::Motion::quick(),
    );
    let mut painter = ui.painter();
    painter.rect(rect, Corners::all(5.0), Color::from_rgb_f32(color));
    painter.outlined(
        rect,
        Corners::all(5.0),
        Fill::None,
        Border {
            width: 1.0,
            color: Color::white_alpha(0.22 + 0.35 * hover),
        },
        Shadow::NONE,
    );
    response.clicked
}
