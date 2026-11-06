use std::sync::Arc;

use glam::Vec2;

use crate::edit::Outcome;
use crate::geometry::{Corners, Rect};
use crate::input::Button;
use crate::registry::Cursor;
use crate::style::Shadow;
use crate::text::{TextLayout, TextStyle};
use crate::theme::Role;
use crate::ui::{Response, Ui};

const FIELD_PAD: f32 = 8.0;

pub fn text_field(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    style: TextStyle,
) -> Option<(Outcome, String)> {
    let key = ui.id(id);
    if !ui.is_editing(key) {
        return None;
    }
    ui.touch_edit();
    let response = ui.interact(key, rect);
    ui.focusable(key, rect, Corners::all(10.0));
    if response.hovered {
        ui.set_cursor(Cursor::Text);
    }
    let time = ui.time();
    let pointer = ui.input().position();
    let primary = ui.input().button(Button::Primary);
    let modifiers = ui.input().modifiers();
    let preedit = ui.input().preedit().map(str::to_string).unwrap_or_default();
    let offset = field_offset(ui, style, rect, &preedit);
    let outcome = match ui.poll_edit() {
        Some(outcome) => Some(outcome),
        None if primary.pressed => {
            if !rect.contains(pointer) {
                Some(Outcome::Commit)
            } else if preedit.is_empty() {
                let count = ui.edit_mut().map_or(1, |edit| edit.click(pointer, time));
                let caret = caret_at(ui, style, rect, offset, pointer.x);
                if let Some(edit) = ui.edit_mut() {
                    match count {
                        2 => {
                            let range = edit.word(caret);
                            edit.select(range, time);
                        }
                        3 => edit.select_all(time),
                        _ => edit.place(caret, modifiers.shift, time),
                    }
                }
                None
            } else {
                None
            }
        }
        None if primary.down && response.dragging => {
            let caret = caret_at(ui, style, rect, offset, pointer.x);
            if let Some(edit) = ui.edit_mut() {
                edit.place(caret, true, time);
            }
            None
        }
        None => None,
    };
    let snapshot = ui.edit().map(|edit| {
        (
            edit.text().to_string(),
            edit.caret(),
            edit.anchor(),
            edit.caret_visible(time),
        )
    });
    if outcome.is_some() {
        ui.end_edit();
    }
    let (text, caret, anchor, blinking) = snapshot?;
    paint_field(
        ui,
        key,
        &response,
        rect,
        style,
        Field {
            text: &text,
            caret,
            anchor,
            blinking,
            preedit: &preedit,
        },
    );
    outcome.map(|outcome| (outcome, text))
}

struct Field<'a> {
    text: &'a str,
    caret: usize,
    anchor: usize,
    blinking: bool,
    preedit: &'a str,
}

fn field_offset(ui: &mut Ui, style: TextStyle, rect: Rect, preedit: &str) -> f32 {
    let Some(edit) = ui.edit() else {
        return 0.0;
    };
    let text = edit.text().to_string();
    let composition = format!("{}{}", prefix(&text, edit.caret()), preedit);
    let caret_x = text_width(ui, style, &composition);
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0)).width();
    (caret_x - inner + 2.0).max(0.0)
}

fn paint_field(
    ui: &mut Ui,
    key: u64,
    response: &Response,
    rect: Rect,
    style: TextStyle,
    field: Field<'_>,
) {
    let Field {
        text,
        caret,
        anchor,
        blinking,
        preedit,
    } = field;
    let look = ui.surface(key, Role::Field, response);
    let palette = ui.theme().palette;
    let radius = 10.0;
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let offset_of = |index: usize| prefix(text, index).len();
    let display = if preedit.is_empty() {
        text.to_string()
    } else {
        let mut display = String::with_capacity(text.len() + preedit.len());
        display.push_str(prefix(text, caret));
        display.push_str(preedit);
        display.push_str(&text[offset_of(caret)..]);
        display
    };
    let composed = preedit.chars().count();
    let display_caret = caret + composed;
    let caret_x = text_width(ui, style, prefix(&display, display_caret));
    let offset = (caret_x - inner.width() + 2.0).max(0.0);
    let layout = ui.layout_text(&display, style, 0.0);
    let origin = Vec2::new(
        inner.min.x - offset,
        inner.min.y + (inner.height() - layout.size.y) * 0.5,
    );
    let height = (layout.size.y * 0.86).max(8.0);
    let top = origin.y + (layout.size.y - height) * 0.5;
    let selection = (preedit.is_empty() && caret != anchor).then(|| {
        let (start, end) = (caret.min(anchor), caret.max(anchor));
        (
            text_width(ui, style, prefix(text, start)),
            text_width(ui, style, prefix(text, end)),
        )
    });
    let composition =
        (composed > 0).then(|| (text_width(ui, style, prefix(&display, caret)), caret_x));
    let accent = palette.accent;
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::all(radius),
        look.fill,
        look.border,
        Shadow::NONE,
    );
    let clip = painter.clip();
    painter.push_clip(inner, Corners::ZERO);
    if let Some((left, right)) = selection {
        painter.rect(
            Rect::from_min_max(
                Vec2::new(inner.min.x - offset + left, top),
                Vec2::new(inner.min.x - offset + right, top + height),
            ),
            Corners::ZERO,
            accent.with_alpha(0.28),
        );
    }
    if let Some((left, right)) = composition {
        painter.rect(
            Rect::from_min_max(
                Vec2::new(inner.min.x - offset + left, top + height + 2.0),
                Vec2::new(inner.min.x - offset + right, top + height + 3.2),
            ),
            Corners::ZERO,
            accent,
        );
    }
    painter.glyphs(&layout, origin, style.color);
    if blinking {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(inner.min.x - offset + caret_x, top),
                Vec2::new(1.4, height),
            ),
            Corners::ZERO,
            accent,
        );
    }
    painter.pop_clip(clip);
    ui.set_edit_caret(Rect::from_min_size(
        Vec2::new(inner.min.x - offset + caret_x, top),
        Vec2::new(1.4, height),
    ));
}

pub fn text_area(ui: &mut Ui, id: &str, rect: Rect, style: TextStyle) -> Option<(Outcome, String)> {
    let key = ui.id(id);
    if !ui.is_editing(key) {
        return None;
    }
    ui.touch_edit();
    let response = ui.interact(key, rect);
    let radius = 10.0;
    ui.focusable(key, rect, Corners::all(radius));
    if response.hovered {
        ui.set_cursor(Cursor::Text);
    }
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let line_height = style.font.leading.max(style.font.size * 1.28);
    let scroll_key = key ^ 0x2f6e_9a3d_1c4b_8a57;
    let mut scroll = ui.value_of(scroll_key).unwrap_or(0.0).max(0.0);
    let time = ui.time();
    let pointer = ui.input().position();
    let primary = ui.input().button(Button::Primary);
    let chars = ui
        .edit()
        .map(|edit| edit.text().chars().collect::<Vec<_>>())
        .unwrap_or_default();
    let spans = line_spans(&chars);
    if response.hovered && ui.input().wheel().y != 0.0 {
        scroll -= ui.input().wheel().y * line_height * 3.0;
    }
    let mut commit = false;
    if primary.pressed {
        if !rect.contains(pointer) {
            commit = true;
        } else {
            let caret = area_caret_at(
                ui,
                style,
                AreaText {
                    inner,
                    chars: &chars,
                    spans: &spans,
                    line_height,
                    scroll,
                },
                pointer,
            );
            let count = ui.edit_mut().map_or(1, |edit| edit.click(pointer, time));
            if let Some(edit) = ui.edit_mut() {
                match count {
                    2 => {
                        let range = edit.word(caret);
                        edit.select(range, time);
                    }
                    3 => edit.select_all(time),
                    _ => edit.place(caret, false, time),
                }
            }
        }
    } else if primary.down && response.dragging {
        let caret = area_caret_at(
            ui,
            style,
            AreaText {
                inner,
                chars: &chars,
                spans: &spans,
                line_height,
                scroll,
            },
            pointer,
        );
        if let Some(edit) = ui.edit_mut() {
            edit.place(caret, true, time);
        }
    }
    let outcome = if commit {
        Some(Outcome::Commit)
    } else {
        ui.poll_edit()
    };
    let snapshot = ui.edit().map(|edit| {
        (
            edit.text().to_string(),
            edit.caret(),
            edit.anchor(),
            edit.caret_visible(time),
        )
    });
    if outcome.is_some() {
        ui.end_edit();
    }
    let (text, caret, anchor, blinking) = snapshot?;
    let chars = text.chars().collect::<Vec<_>>();
    let spans = line_spans(&chars);
    let (caret_line, caret_x, caret_y) =
        area_caret_xy(ui, style, &chars, &spans, caret, line_height);
    let max_scroll = (spans.len() as f32 * line_height - inner.height()).max(0.0);
    if caret_y < scroll {
        scroll = caret_y;
    } else if caret_y + line_height > scroll + inner.height() {
        scroll = caret_y + line_height - inner.height();
    }
    scroll = scroll.clamp(0.0, max_scroll);
    ui.store_value(scroll_key, scroll);
    let selection = (anchor.min(caret), anchor.max(caret));
    let mut select_rects = Vec::new();
    if selection.0 != selection.1 {
        for (index, (start, end)) in spans.iter().enumerate() {
            let low = selection.0.max(*start);
            let high = selection.1.min(*end);
            if low < high {
                let x1 = line_width(ui, style, &chars, *start, low);
                let x2 = line_width(ui, style, &chars, *start, high);
                select_rects.push(Rect::from_min_size(
                    Vec2::new(
                        inner.min.x + x1,
                        inner.min.y - scroll + index as f32 * line_height,
                    ),
                    Vec2::new(x2 - x1, line_height),
                ));
            }
        }
    }
    let layouts = spans
        .iter()
        .map(|(start, end)| {
            let line = chars[*start..*end].iter().collect::<String>();
            ui.layout_text(&line, style, 0.0)
        })
        .collect::<Vec<Arc<TextLayout>>>();
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    let clip = painter.clip();
    painter.push_clip(inner, Corners::all(radius));
    for selection in select_rects {
        painter.rect(selection, Corners::ZERO, palette.accent.with_alpha(0.28));
    }
    let first = (scroll / line_height).floor().max(0.0) as usize;
    let last = ((scroll + inner.height()) / line_height).ceil() as usize + 1;
    for (index, layout) in layouts.iter().enumerate().take(last).skip(first) {
        painter.text(
            layout,
            Vec2::new(
                inner.min.x,
                inner.min.y - scroll + index as f32 * line_height,
            ),
            style.color,
        );
    }
    let caret_rect = Rect::from_min_size(
        Vec2::new(
            inner.min.x + caret_x,
            inner.min.y - scroll + caret_line as f32 * line_height + 1.0,
        ),
        Vec2::new(1.4, (line_height - 2.0).max(1.0)),
    );
    if blinking {
        painter.rect(caret_rect, Corners::ZERO, palette.accent);
    }
    painter.pop_clip(clip);
    ui.set_edit_caret(caret_rect);
    outcome.map(|outcome| (outcome, text))
}

fn line_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, character) in chars.iter().enumerate() {
        if *character == '\n' {
            spans.push((start, index));
            start = index + 1;
        }
    }
    spans.push((start, chars.len()));
    spans
}

fn line_width(ui: &mut Ui, style: TextStyle, chars: &[char], start: usize, end: usize) -> f32 {
    if start >= end {
        return 0.0;
    }
    let line = chars[start..end].iter().collect::<String>();
    ui.measure_text(&line, style).x
}

fn area_caret_xy(
    ui: &mut Ui,
    style: TextStyle,
    chars: &[char],
    spans: &[(usize, usize)],
    caret: usize,
    line_height: f32,
) -> (usize, f32, f32) {
    let caret = caret.min(chars.len());
    let mut line = spans.len() - 1;
    for (index, (_, end)) in spans.iter().enumerate() {
        if caret <= *end {
            line = index;
            break;
        }
    }
    let start = spans[line].0;
    let x = line_width(ui, style, chars, start, caret);
    (line, x, line as f32 * line_height)
}

struct AreaText<'a> {
    inner: Rect,
    chars: &'a [char],
    spans: &'a [(usize, usize)],
    line_height: f32,
    scroll: f32,
}

fn area_caret_at(ui: &mut Ui, style: TextStyle, area: AreaText<'_>, point: Vec2) -> usize {
    let line = (((point.y - area.inner.min.y + area.scroll) / area.line_height).floor() as isize)
        .clamp(0, area.spans.len() as isize - 1) as usize;
    let (start, end) = area.spans[line];
    let x = point.x - area.inner.min.x;
    let mut best = start;
    let mut distance = f32::INFINITY;
    for column in 0..=end - start {
        let width = line_width(ui, style, area.chars, start, start + column);
        let candidate = (width - x).abs();
        if candidate < distance {
            distance = candidate;
            best = start + column;
        }
    }
    best
}

fn caret_at(ui: &mut Ui, style: TextStyle, rect: Rect, offset: f32, x: f32) -> usize {
    let Some(edit) = ui.edit() else {
        return 0;
    };
    let text = edit.text().to_string();
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let x = x - inner.min.x + offset;
    let mut best = 0usize;
    let mut distance = f32::INFINITY;
    for index in 0..=text.chars().count() {
        let width = text_width(ui, style, prefix(&text, index));
        let candidate = (width - x).abs();
        if candidate < distance {
            distance = candidate;
            best = index;
        }
    }
    best
}

fn text_width(ui: &mut Ui, style: TextStyle, text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.measure_text(text, style).x
}

fn prefix(text: &str, index: usize) -> &str {
    let offset = text
        .char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset);
    &text[..offset]
}
