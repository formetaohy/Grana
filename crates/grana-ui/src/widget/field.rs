use glam::Vec2;

use crate::content::{Content, FieldContent};
use crate::edit::Edit;
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::style::{Border, State};
use crate::text::TextStyle;
use crate::theme::Role;

use super::PaintInfo;

const FIELD_PAD: f32 = 10.0;

pub fn text_field<'a, A>(
    value: &str,
    style: TextStyle,
    width: f32,
    multiline: bool,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(
            Length::Fixed(width),
            Length::Fixed(if multiline { 96.0 } else { 30.0 }),
        ),
        ..Layout::row()
    })
    .with_content(Content::Field(FieldContent {
        value: value.to_string(),
        style,
        multiline,
        radius: 10.0,
        editing: false,
        select_all: false,
        error: false,
    }))
}

pub(super) fn measure_field(field: &FieldContent, layout: Layout, available: Vec2) -> Vec2 {
    let width = match layout.size.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => available.x.min(maximum),
    };
    let height = match layout.size.height {
        Length::Auto | Length::Fill => available.y,
        other => other.resolve(available.y),
    };
    let _ = field;
    Vec2::new(width, height)
}

pub(crate) fn caret_at(
    edit: &Edit,
    preedit: Option<&str>,
    style: TextStyle,
    multiline: bool,
    rect: Rect,
    pointer: Vec2,
    layout: &mut crate::text::Text,
) -> usize {
    let text = edit.text();
    let caret = edit.caret();
    let inner = inner(rect);
    let preedit_width = preedit.map_or(0.0, |composed| width(composed, style, layout));
    if !multiline {
        let caret_x = width(prefix(text, caret), style, layout);
        let shift = shift(caret_x, preedit_width, inner.width());
        return column(text, pointer.x - inner.min.x + shift, style, layout);
    }
    let line_height = style.font.leading.max(style.font.size * 1.28);
    let row = ((pointer.y - inner.min.y) / line_height).floor().max(0.0) as usize;
    let chars = text.chars().collect::<Vec<_>>();
    let spans = line_spans(&chars);
    let (_, line_x) = caret_x_of(style, &chars, &spans, caret, layout);
    let shift = shift(line_x, preedit_width, inner.width());
    let x = pointer.x - inner.min.x + shift;
    let mut base = 0usize;
    for (index, (start, end)) in spans.iter().enumerate() {
        if index == row {
            let line = chars[*start..*end].iter().collect::<String>();
            return base + column(&line, x, style, layout);
        }
        base += end - start + 1;
    }
    text.chars().count()
}

fn inner(rect: Rect) -> Rect {
    rect.shrink2(Vec2::new(FIELD_PAD, 4.0))
}

fn shift(caret_x: f32, preedit_width: f32, width: f32) -> f32 {
    (caret_x + preedit_width + 2.0 - width).max(0.0)
}

fn caret_x_of(
    style: TextStyle,
    chars: &[char],
    spans: &[(usize, usize)],
    caret: usize,
    layout: &mut crate::text::Text,
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
    let text = chars[start..caret].iter().collect::<String>();
    (line, width(&text, style, layout))
}

fn column(line: &str, x: f32, style: TextStyle, layout: &mut crate::text::Text) -> usize {
    let count = line.chars().count();
    let (mut low, mut high) = (0usize, count);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if width(prefix(line, middle), style, layout) <= x {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let before = width(prefix(line, low), style, layout);
    if low < count {
        let after = width(prefix(line, low + 1), style, layout);
        if (after - x).abs() < (before - x).abs() {
            return low + 1;
        }
    }
    low
}

fn width(text: &str, style: TextStyle, layout: &mut crate::text::Text) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    layout
        .layout(
            text,
            style.font,
            0.0,
            false,
            false,
            crate::text::Align::Start,
        )
        .size
        .x
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
    } else if runtime.edit.is_some() {
        State::Selected
    } else if info.press > 0.5 {
        State::Pressed
    } else if info.hover > 0.5 {
        State::Hover
    } else {
        State::Rest
    };
    let look = info.theme.surface(Role::Field, state);
    let palette = info.theme.palette;
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 4.0));
    let border = if field.error {
        Border {
            width: look.border.width.max(1.5),
            color: palette.error,
        }
    } else {
        look.border
    };
    painter.outlined(rect, Corners::all(radius), look.fill, border);
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
        let preedit = info.preedit.filter(|composed| !composed.is_empty());
        let preedit_width = preedit.map_or(0.0, |composed| painter.measure(composed, style).x);
        let mut offset = shift(caret_x, preedit_width, inner.width());
        if field.multiline {
            let line_height = style.font.leading.max(style.font.size * 1.28);
            let chars: Vec<char> = text.chars().collect();
            let spans = line_spans(&chars);
            let (_, line_x) = caret_xy(painter, style, &chars, &spans, caret);
            offset = shift(line_x, preedit_width, inner.width());
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
                            palette.primary.with_alpha(0.28),
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
                    palette.primary.with_alpha(0.8),
                );
            }
            let caret_rect = Rect::from_min_size(
                Vec2::new(
                    inner.min.x + caret_x + preedit_width - offset,
                    inner.min.y + caret_line as f32 * line_height + 1.0,
                ),
                Vec2::new(1.6, (line_height - 2.0).max(1.0)),
            );
            if edit.caret_visible(info.time) {
                painter.rect(caret_rect, Corners::ZERO, palette.primary);
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
                palette.primary.with_alpha(0.28),
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
                palette.primary.with_alpha(0.8),
            );
        }
        let caret_rect = Rect::from_min_size(
            Vec2::new(
                inner.min.x + caret_x + preedit_width - offset,
                inner.min.y + 2.0,
            ),
            Vec2::new(1.6, inner.height() - 4.0),
        );
        if edit.caret_visible(info.time) {
            painter.rect(caret_rect, Corners::ZERO, palette.primary);
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
