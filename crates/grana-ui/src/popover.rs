use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::icon::{self, Icon};
use crate::motion::Motion;
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role};
use crate::ui::Ui;
use crate::widget;

pub fn popover(
    ui: &mut Ui,
    id: &str,
    open: bool,
    anchor: Rect,
    width: f32,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let key = ui.id(id);
    if !open {
        ui.glass_collapse(key);
        ui.replay_popup(key);
        return false;
    }
    let gap = 16.0;
    let window = ui.size();
    let margin = 12.0;
    let x = anchor.min.x.min(window.x - width - margin).max(margin);
    let room = (window.y - anchor.max.y - gap - margin).max(160.0);
    let padding = 8.0;
    let area = Rect::from_min_size(Vec2::new(x, anchor.max.y + gap), Vec2::new(width, room));
    let start = ui.popup_len();
    ui.push_id(id);
    let height = ui.popup_scope(area.shrink(padding), content);
    ui.pop_id();
    let body = Rect::from_min_size(
        area.min,
        Vec2::new(width, (height + padding * 2.0).min(room + padding * 2.0)),
    );
    let alpha = ui.glass_expand(key, anchor, body, GlassStyle::popover());
    ui.record_popup(key, start);
    ui.fade_popup(start, alpha);
    let pointer = ui.input().position();
    let pressed = ui.input().button(crate::input::Button::Primary).pressed;
    if pressed && !body.contains(pointer) && !anchor.contains(pointer) {
        ui.close_popup();
        return false;
    }
    true
}

pub fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    width: f32,
    items: &[(T, String)],
    selected: Option<T>,
) -> Option<T> {
    let key = ui.id(id);
    let label = selected
        .and_then(|value| {
            items
                .iter()
                .find(|(candidate, _)| *candidate == value)
                .map(|(_, text)| text.clone())
        })
        .unwrap_or_else(|| "\u{2014}".to_string());
    let metrics = ui.theme().metrics;
    let font = ui.theme().font(Level::Callout);
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let mut response = ui.interact(key, rect);
    let corners = Corners::all(10.0);
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    if response.clicked {
        ui.toggle_popup(key);
    }
    let press = ui.animate(
        key ^ 0x31,
        if response.active { 1.0 } else { 0.0 },
        if response.active {
            Motion::response(0.22, 0.0)
        } else {
            Motion::bouncy()
        },
    );
    let body = rect.expand(1.2 * press);
    let surface = ui.surface(key, Role::Field, &response);
    let chevron_color = ui.theme().palette.text_tertiary;
    let text_style = TextStyle::new(font, surface.text).truncating();
    let mut painter = ui.painter();
    painter.outlined(
        body,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    painter.text_in(
        Rect::from_min_max(
            body.min + Vec2::new(12.0, 0.0),
            Vec2::new(body.max.x - 24.0, body.max.y),
        ),
        &label,
        text_style,
        Align::Center,
    );
    icon::paint(
        Icon::ChevronDown,
        &mut painter,
        Vec2::new(body.max.x - 13.0, body.center().y),
        10.0,
        chevron_color,
        1.6,
    );
    let mut chosen = None;
    let open = ui.popup_open(key);
    let highlight_key = key ^ 0x51ed_2701_9e37_79b9;
    if response.clicked && ui.popup_open(key) {
        let index = selected
            .and_then(|value| items.iter().position(|(item, _)| *item == value))
            .unwrap_or(0);
        ui.store_value(highlight_key, index as f32);
    }
    let mut highlight = ui
        .value_of(highlight_key)
        .map_or(0, |value| value.round().max(0.0) as usize);
    if highlight >= items.len() {
        highlight = 0;
    }
    if open && !items.is_empty() {
        let keys = ui.input().keys().to_vec();
        for event in keys {
            if !event.pressed || event.repeat {
                continue;
            }
            match event.key {
                Some(crate::input::Key::Up) => {
                    highlight = (highlight + items.len() - 1) % items.len();
                    ui.store_value(highlight_key, highlight as f32);
                }
                Some(crate::input::Key::Down) => {
                    highlight = (highlight + 1) % items.len();
                    ui.store_value(highlight_key, highlight as f32);
                }
                Some(crate::input::Key::Enter | crate::input::Key::Space) => {
                    chosen = Some(items[highlight].0);
                }
                _ => {}
            }
        }
    }
    let scroll = ui.id(&format!("{id}.scroll"));
    let stays = popover(ui, id, open, rect, width.max(168.0), |ui| {
        let room = ui.available().y;
        let content = items.len() as f32 * MENU_ROW;
        if content > room {
            ui.scroll_axis(
                scroll,
                crate::ui::Axis::Column,
                crate::ui::Length::Fixed(room),
                Some(content),
                |ui| {
                    if let Some(value) = rows(ui, items, selected, highlight) {
                        chosen = Some(value);
                    }
                },
            );
        } else if let Some(value) = rows(ui, items, selected, highlight) {
            chosen = Some(value);
        }
    });
    if chosen.is_some() || (!stays && open) {
        ui.close_popup();
    }
    chosen
}

const MENU_ROW: f32 = 32.0;

fn rows<T: Copy + PartialEq>(
    ui: &mut Ui,
    items: &[(T, String)],
    selected: Option<T>,
    highlight: usize,
) -> Option<T> {
    let mut chosen = None;
    for (index, (value, text)) in items.iter().enumerate() {
        if menu_row_highlighted(
            ui,
            index,
            text,
            selected == Some(*value),
            index == highlight,
        ) {
            chosen = Some(*value);
        }
    }
    chosen
}

pub fn menu_row(ui: &mut Ui, index: usize, label: &str, checked: bool) -> bool {
    menu_row_highlighted(ui, index, label, checked, false)
}

fn menu_row_highlighted(
    ui: &mut Ui,
    index: usize,
    label: &str,
    checked: bool,
    highlighted: bool,
) -> bool {
    let key = ui.id(&format!("menu.row.{index}"));
    let height = MENU_ROW;
    let rect = ui.allocate(Vec2::new(ui.available().x, height));
    let response = ui.interact(key, rect);
    let press = ui.animate(
        key,
        if response.hovered || highlighted {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout);
    let text_color = if checked {
        palette.text_primary
    } else {
        palette
            .text_primary
            .lerp(palette.text_secondary, 1.0 - press)
    };
    let mut painter = ui.painter();
    if press > 0.01 {
        painter.rect(
            rect.shrink2(Vec2::new(2.0, 0.0)),
            Corners::all(10.0),
            palette.selected.amplify(0.9 * press),
        );
    }
    if checked {
        icon::paint(
            Icon::Check,
            &mut painter,
            Vec2::new(rect.min.x + 12.0, rect.center().y),
            12.0,
            palette.text_primary,
            1.7,
        );
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 26.0, rect.min.y), rect.max),
        label,
        TextStyle::new(font, text_color),
        Align::Center,
    );
    response.clicked
}

pub fn action_row(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    label: &str,
    shortcut: Option<&str>,
) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::new(ui.available().x, MENU_ROW));
    let response = ui.interact(key, rect);
    let press = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout);
    let caption = ui.theme().font(Level::Caption);
    let shortcut_width = shortcut.map_or(8.0, |text| {
        18.0 + ui
            .measure_text(text, TextStyle::new(caption, palette.text_tertiary))
            .x
    });
    let mut painter = ui.painter();
    if press > 0.01 {
        painter.rect(
            rect.shrink2(Vec2::new(2.0, 0.0)),
            Corners::all(10.0),
            palette.selected.amplify(0.9 * press),
        );
    }
    icon::paint(
        symbol,
        &mut painter,
        Vec2::new(rect.min.x + 14.0, rect.center().y),
        14.0,
        palette.text_secondary.lerp(palette.text_primary, press),
        1.5,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(rect.min.x + 28.0, rect.min.y),
            Vec2::new(rect.max.x - shortcut_width, rect.max.y),
        ),
        label,
        TextStyle::new(font, palette.text_primary),
        Align::Center,
    );
    if let Some(shortcut) = shortcut {
        painter.text_in(
            rect.shrink2(Vec2::new(10.0, 0.0)),
            shortcut,
            TextStyle::new(caption, palette.text_tertiary).aligned(Align::End),
            Align::Center,
        );
    }
    response.clicked
}

pub fn separator(ui: &mut Ui) {
    let rect = ui.allocate(Vec2::new(ui.available().x, 9.0));
    let separator = ui.theme().palette.separator;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 10.0, rect.center().y),
            Vec2::new(rect.width() - 20.0, 1.0),
        ),
        Corners::ZERO,
        separator,
    );
}

pub fn disclosure_row(ui: &mut Ui, id: &str, label: &str, open: bool) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::new(ui.available().x, 28.0));
    let response = ui.interact(key, rect);
    let press = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout).medium();
    let color = palette.text_secondary.lerp(palette.text_primary, press);
    let rotation = ui.animate(key ^ 0x19, if open { 1.0 } else { 0.0 }, Motion::snappy());
    let mut painter = ui.painter();
    if press > 0.01 {
        painter.rect(
            rect.shrink2(Vec2::new(2.0, 1.0)),
            Corners::all(8.0),
            palette.selected.amplify(0.9 * press),
        );
    }
    let centre = Vec2::new(rect.min.x + 11.0, rect.center().y);
    let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * rotation);
    let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.2;
    painter.segment(tip + Vec2::new(-2.0, -1.3), tip, 1.5, palette.text_tertiary);
    painter.segment(tip, tip + Vec2::new(2.0, -1.3), 1.5, palette.text_tertiary);
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 22.0, rect.min.y), rect.max),
        label,
        TextStyle::new(font, color),
        Align::Center,
    );
    response.clicked
}

pub fn color_picker(
    ui: &mut Ui,
    id: &str,
    open: bool,
    anchor: Rect,
    color: [f32; 3],
) -> Option<[f32; 3]> {
    let size = 176.0;
    let inner = 8.0;
    if !open {
        popover(ui, id, false, anchor, size + inner * 2.0, |_| {});
        return None;
    }
    let key = ui.id(id);
    let stored_hue = ui.value_of(key).unwrap_or(-1.0);
    let (mut hue, saturation, value) = rgb_to_hsv(color);
    if stored_hue >= 0.0 {
        hue = stored_hue;
    }
    let mut changed = None;
    let square = Rect::from_min_size(Vec2::ZERO, Vec2::splat(size));
    let visible = popover(ui, id, open, anchor, size + inner * 2.0, |ui| {
        let origin = ui.cursor_position();
        let square = square.translate(origin);
        let cells = 14;
        let cell = size / cells as f32;
        let mut colors = Vec::with_capacity((cells * cells) as usize);
        for row in 0..cells {
            for column in 0..cells {
                let s = (column as f32 + 0.5) / cells as f32;
                let v = 1.0 - (row as f32 + 0.5) / cells as f32;
                colors.push(Color::from_rgb_f32(hsv_to_rgb(hue, s, v)));
            }
        }
        {
            let mut painter = ui.painter();
            for (index, tint) in colors.iter().enumerate() {
                let row = (index as u32) / cells;
                let column = (index as u32) % cells;
                painter.rect(
                    Rect::from_min_size(
                        square.min + Vec2::new(column as f32 * cell, row as f32 * cell),
                        Vec2::splat(cell + 0.5),
                    ),
                    Corners::ZERO,
                    *tint,
                );
            }
            let marker = Vec2::new(
                square.min.x + size * saturation,
                square.min.y + size * (1.0 - value),
            );
            painter.ring(marker, 6.0, 1.8, 0.0, std::f32::consts::TAU, Color::WHITE);
            painter.ring(
                marker,
                7.8,
                1.0,
                0.0,
                std::f32::consts::TAU,
                Color::black_alpha(0.4),
            );
        }
        let square_key = ui.id("square");
        let square_response = ui.interact(square_key, square);
        if square_response.active
            && let Some(pointer) = ui.pointer_in(square)
        {
            let s = ((pointer.x - square.min.x) / size).clamp(0.0, 1.0);
            let v = (1.0 - (pointer.y - square.min.y) / size).clamp(0.0, 1.0);
            ui.store_value(key, hue);
            changed = Some(hsv_to_rgb(hue, s, v));
        }
        ui.space(size + 8.0);
        let strip = Rect::from_min_size(ui.cursor_position(), Vec2::new(ui.available().x, 14.0));
        let stops = 24;
        {
            let mut painter = ui.painter();
            let step = strip.width() / stops as f32;
            for index in 0..stops {
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(strip.min.x + index as f32 * step, strip.min.y),
                        Vec2::new(step + 0.5, strip.height()),
                    ),
                    Corners::ZERO,
                    Color::from_rgb_f32(hsv_to_rgb(index as f32 / (stops - 1) as f32, 1.0, 1.0)),
                );
            }
            let marker = Vec2::new(strip.min.x + strip.width() * hue, strip.center().y);
            painter.ring(marker, 7.0, 1.8, 0.0, std::f32::consts::TAU, Color::WHITE);
        }
        let strip_response = ui.interact(ui.id("hue"), strip);
        if strip_response.active
            && let Some(pointer) = ui.pointer_in(strip)
        {
            let hue = ((pointer.x - strip.min.x) / strip.width()).clamp(0.0, 1.0);
            ui.store_value(key, hue);
            changed = Some(hsv_to_rgb(hue, saturation, value));
        }
        ui.space(22.0);
    });
    if !visible {
        ui.close_popup();
    }
    if changed.is_some() {
        ui.store_value(key, hue);
    }
    changed
}

pub fn color_field(ui: &mut Ui, id: &str, color: [f32; 3], size: f32) -> Option<[f32; 3]> {
    let key = ui.id(id);
    let anchor = ui.cursor_position();
    let clicked = widget::color_swatch(ui, id, color, size);
    let rect = Rect::from_min_size(anchor, Vec2::splat(size));
    if clicked {
        ui.toggle_popup(key);
    }
    let open = ui.popup_open(key);
    color_picker(ui, id, open, rect, color)
}

pub fn swatch_button(
    ui: &mut Ui,
    id: &str,
    color: [f32; 3],
    open: bool,
    size: f32,
) -> ([f32; 3], bool) {
    let anchor = ui.cursor_position();
    let clicked = widget::color_swatch(ui, id, color, size);
    let rect = Rect::from_min_size(anchor, Vec2::splat(size));
    if clicked {
        ui.toggle_popup(ui.id(id));
    }
    let open = open || ui.popup_open(ui.id(id));
    let changed = color_picker(ui, id, open, rect, color);
    (changed.unwrap_or(color), open)
}

fn rgb_to_hsv(color: [f32; 3]) -> (f32, f32, f32) {
    let maximum = color[0].max(color[1]).max(color[2]);
    let minimum = color[0].min(color[1]).min(color[2]);
    let delta = maximum - minimum;
    let value = maximum;
    let saturation = if maximum <= 0.0 { 0.0 } else { delta / maximum };
    let hue = if delta <= 0.0 {
        0.0
    } else if maximum == color[0] {
        ((color[1] - color[2]) / delta).rem_euclid(6.0) / 6.0
    } else if maximum == color[1] {
        ((color[2] - color[0]) / delta + 2.0) / 6.0
    } else {
        ((color[0] - color[1]) / delta + 4.0) / 6.0
    };
    (hue, saturation, value)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let hue = (hue.rem_euclid(1.0)) * 6.0;
    let index = hue.floor() as i32;
    let fraction = hue - index as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    match index {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}
