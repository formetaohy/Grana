use glam::Vec2;

use crate::content::{Content, FieldContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::style::{Shadow, State};
use crate::text::TextStyle;
use crate::theme::Role;

use super::PaintInfo;

const FIELD_PAD: f32 = 8.0;

pub fn text_field<'a, A>(
    value: &str,
    style: TextStyle,
    width: f32,
    multiline: bool,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(
            Length::Fixed(width),
            Length::Fixed(if multiline { 96.0 } else { 24.0 }),
        ),
        ..Layout::row()
    })
    .with_content(Content::Field(FieldContent {
        value: value.to_string(),
        style,
        multiline,
        radius: 6.0,
        editing: false,
    }))
}

pub(super) fn measure_field(field: &FieldContent, layout: Layout, available: Vec2) -> Vec2 {
    let width = match layout.size.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => maximum.min(available.x),
    };
    let height = match layout.size.height {
        Length::Auto | Length::Fill => available.y,
        other => other.resolve(available.y),
    };
    let _ = field;
    Vec2::new(width, height)
}

pub(crate) fn caret_at(
    field: &FieldContent,
    rect: Rect,
    pointer: Vec2,
    text: &mut crate::text::Text,
) -> usize {
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let style = field.style;
    let x = pointer.x - inner.min.x;
    if !field.multiline {
        return column(field.value.as_str(), x, style, text);
    }
    let line_height = style.font.leading.max(style.font.size * 1.28);
    let row = ((pointer.y - inner.min.y) / line_height).floor().max(0.0) as usize;
    let mut offset = 0usize;
    for (index, line) in field.value.split('\n').enumerate() {
        if index == row {
            return offset + column(line, x, style, text);
        }
        offset += line.chars().count() + 1;
    }
    field.value.chars().count()
}

fn column(line: &str, x: f32, style: TextStyle, text: &mut crate::text::Text) -> usize {
    let mut best = 0usize;
    let mut distance = f32::INFINITY;
    for index in 0..=line.chars().count() {
        let prefix = prefix(line, index);
        let width = if prefix.is_empty() {
            0.0
        } else {
            text.layout(
                prefix,
                style.font,
                0.0,
                false,
                false,
                crate::text::Align::Start,
            )
            .size
            .x
        };
        let candidate = (width - x).abs();
        if candidate < distance {
            distance = candidate;
            best = index;
        }
    }
    best
}

fn prefix(text: &str, index: usize) -> &str {
    let offset = text
        .char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset);
    &text[..offset]
}

pub(super) fn paint_field(
    field: &FieldContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) -> Option<Rect> {
    let rect = info.rect;
    let radius = field.radius;
    let state = if !info.enabled {
        State::Disabled
    } else if info.press > 0.5 {
        State::Pressed
    } else if info.hover > 0.5 {
        State::Hover
    } else {
        State::Rest
    };
    let mut look = info.theme.surface(Role::Field, state);
    let palette = info.theme.palette;
    if runtime.edit.is_some() {
        look.border.color = palette.accent.with_alpha(0.65);
    }
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    painter.outlined(
        rect,
        Corners::all(radius),
        look.fill,
        look.border,
        Shadow::NONE,
    );
    let clip = painter.clip();
    painter.push_clip(inner, Corners::ZERO);
    let style = field.style;
    if let Some(edit) = &runtime.edit {
        let text = edit.text();
        let caret = edit.caret();
        let anchor = edit.anchor();
        let before = prefix(text, caret);
        let caret_x = if before.is_empty() {
            0.0
        } else {
            painter.measure(before, style).x
        };
        let mut offset = (caret_x - inner.width() + 2.0).max(0.0);
        let preedit = info.preedit.filter(|composed| !composed.is_empty());
        let preedit_width = preedit.map_or(0.0, |composed| painter.measure(composed, style).x);
        if !field.multiline {
            offset = (caret_x + preedit_width - inner.width() + 2.0).max(0.0);
        }
        if field.multiline {
            let line_height = style.font.leading.max(style.font.size * 1.28);
            let chars: Vec<char> = text.chars().collect();
            let spans = line_spans(&chars);
            let (_, line_x) = caret_xy(painter, style, &chars, &spans, caret);
            let mut next = 0.0;
            if line_x < next + 2.0 {
                next = line_x - 2.0;
            } else if line_x + 2.0 + preedit_width > next + inner.width() {
                next = line_x + 2.0 + preedit_width - inner.width();
            }
            offset = next.max(0.0);
            let selection = anchor.min(caret)..anchor.max(caret);
            if selection.start != selection.end {
                for (index, (start, end)) in spans.iter().enumerate() {
                    let low = selection.start.max(*start);
                    let high = selection.end.min(*end);
                    if low < high {
                        let left = line_width(painter, style, &chars, *start, low);
                        let right = line_width(painter, style, &chars, *start, high);
                        painter.rect(
                            Rect::from_min_size(
                                Vec2::new(
                                    inner.min.x + left - offset,
                                    inner.min.y + index as f32 * line_height,
                                ),
                                Vec2::new(right - left, line_height),
                            ),
                            Corners::ZERO,
                            palette.accent.with_alpha(0.28),
                        );
                    }
                }
            }
            for (index, (start, end)) in spans.iter().enumerate() {
                let line = chars[*start..*end].iter().collect::<String>();
                let layout = painter.layout(&line, style, 0.0);
                painter.text(
                    &layout,
                    Vec2::new(
                        inner.min.x - offset,
                        inner.min.y + index as f32 * line_height,
                    ),
                    style.color,
                );
            }
            let (_, caret_x) = caret_xy(painter, style, &chars, &spans, caret);
            let caret_line = caret_line_of(&chars, caret);
            if let Some(composed) = preedit {
                let layout = painter.layout(composed, style, 0.0);
                let origin = Vec2::new(
                    inner.min.x + caret_x - offset,
                    inner.min.y + caret_line as f32 * line_height,
                );
                painter.text(&layout, origin, style.color);
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(origin.x, origin.y + layout.size.y - 1.0),
                        Vec2::new(layout.size.x, 1.0),
                    ),
                    Corners::ZERO,
                    palette.accent.with_alpha(0.8),
                );
            }
            let caret_rect = Rect::from_min_size(
                Vec2::new(
                    inner.min.x + caret_x + preedit_width - offset,
                    inner.min.y + caret_line as f32 * line_height + 1.0,
                ),
                Vec2::new(1.4, (line_height - 2.0).max(1.0)),
            );
            if edit.caret_visible(info.time) {
                painter.rect(caret_rect, Corners::ZERO, palette.accent);
            }
            painter.pop_clip(clip);
            return Some(caret_rect);
        }
        let selection = anchor.min(caret)..anchor.max(caret);
        if selection.start != selection.end {
            let left = text_width(painter, style, prefix(text, selection.start));
            let right = text_width(painter, style, prefix(text, selection.end));
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(inner.min.x + left - offset, inner.min.y + 2.0),
                    Vec2::new(right - left, inner.height() - 4.0),
                ),
                Corners::ZERO,
                palette.accent.with_alpha(0.28),
            );
        }
        let layout = painter.layout(text, style, 0.0);
        let origin = Vec2::new(
            inner.min.x - offset,
            inner.min.y + (inner.height() - layout.size.y) * 0.5,
        );
        painter.text(&layout, origin, style.color);
        if let Some(composed) = preedit {
            let composed = painter.layout(composed, style, 0.0);
            let origin = Vec2::new(
                inner.min.x + caret_x - offset,
                inner.min.y + (inner.height() - composed.size.y) * 0.5,
            );
            painter.text(&composed, origin, style.color);
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(origin.x, origin.y + composed.size.y - 1.0),
                    Vec2::new(composed.size.x, 1.0),
                ),
                Corners::ZERO,
                palette.accent.with_alpha(0.8),
            );
        }
        let caret_rect = Rect::from_min_size(
            Vec2::new(
                inner.min.x + caret_x + preedit_width - offset,
                inner.min.y + 2.0,
            ),
            Vec2::new(1.4, inner.height() - 4.0),
        );
        if edit.caret_visible(info.time) {
            painter.rect(caret_rect, Corners::ZERO, palette.accent);
        }
        painter.pop_clip(clip);
        Some(caret_rect)
    } else {
        let text = &field.value;
        let layout = painter.layout(text, style, inner.width());
        let origin = Vec2::new(
            inner.min.x,
            inner.min.y + (inner.height() - layout.size.y) * 0.5,
        );
        painter.text(&layout, origin, style.color);
        painter.pop_clip(clip);
        None
    }
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

fn caret_line_of(chars: &[char], caret: usize) -> usize {
    chars
        .iter()
        .take(caret.min(chars.len()))
        .filter(|c| **c == '\n')
        .count()
}

fn caret_xy(
    painter: &mut Painter,
    style: TextStyle,
    chars: &[char],
    spans: &[(usize, usize)],
    caret: usize,
) -> (usize, f32) {
    let caret = caret.min(chars.len());
    let mut line = spans.len().saturating_sub(1);
    for (index, (_, end)) in spans.iter().enumerate() {
        if caret <= *end {
            line = index;
            break;
        }
    }
    let start = spans[line].0;
    let x = line_width(painter, style, chars, start, caret);
    (line, x)
}

fn line_width(
    painter: &mut Painter,
    style: TextStyle,
    chars: &[char],
    start: usize,
    end: usize,
) -> f32 {
    if start >= end {
        return 0.0;
    }
    let line = chars[start..end].iter().collect::<String>();
    painter.measure(&line, style).x
}

fn text_width(painter: &mut Painter, style: TextStyle, text: &str) -> f32 {
    if text.is_empty() {
        0.0
    } else {
        painter.measure(text, style).x
    }
}
