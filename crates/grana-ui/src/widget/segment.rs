use glam::Vec2;

use crate::color::Color;
use crate::element::{Content, Element, Layout, Length, SegmentContent, Size, TabsContent};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::node::Runtime;
use crate::paint::Painter;
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Theme};

use super::{PaintInfo, SubGlass};

pub fn segmented<'a, A>(theme: &Theme, options: &[String], selected: usize) -> Element<'a, A> {
    segmented_core(theme, options, selected, false)
}

pub fn segmented_fill<'a, A>(theme: &Theme, options: &[String], selected: usize) -> Element<'a, A> {
    segmented_core(theme, options, selected, true)
}

fn segmented_core<'a, A>(
    theme: &Theme,
    options: &[String],
    selected: usize,
    fill: bool,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(theme.metrics.control_height)),
        ..Layout::row()
    })
    .with_content(Content::Segment(SegmentContent {
        options: options.to_vec(),
        selected,
        fill,
        tabs: false,
        bordered: false,
        font: theme.font(Level::Callout).medium(),
        width: Length::Fill,
    }))
    .focus_ring(Corners::capsule())
    .cursor(crate::input::Cursor::Pointing)
}

pub fn tabs<'a, A>(theme: &Theme, options: &[String], selected: usize) -> Element<'a, A> {
    tab_bar(theme, options, selected, false)
}

pub fn tab_bar<'a, A>(
    theme: &Theme,
    options: &[String],
    selected: usize,
    bordered: bool,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(32.0)),
        ..Layout::row()
    })
    .with_content(Content::Tabs(TabsContent {
        options: options.to_vec(),
        selected,
        bordered,
        font: theme.font(Level::Callout).medium(),
    }))
    .focus_ring(Corners::capsule())
    .cursor(crate::input::Cursor::Pointing)
}

pub(super) fn measure_segment(
    segment: &SegmentContent,
    available: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Vec2 {
    let style = TextStyle::new(segment.font, Color::WHITE);
    let measured = text_widths(&segment.options, text, style);
    let total = if segment.fill {
        available.x
    } else {
        measured.iter().sum::<f32>() + 6.0
    };
    Vec2::new(total, theme.metrics.control_height)
}

pub(super) fn measure_tabs(
    tabs: &TabsContent,
    available: Vec2,
    _text: &mut crate::text::Text,
    _theme: &Theme,
) -> Vec2 {
    let _ = tabs;
    Vec2::new(available.x, 32.0)
}

fn text_widths(options: &[String], text: &mut crate::text::Text, style: TextStyle) -> Vec<f32> {
    options
        .iter()
        .map(|option| {
            text.layout(option, style.font, 0.0, false, false, Align::Start)
                .size
                .x
                + 24.0
        })
        .collect()
}

fn cell_widths(content: &Content, rect: Rect, widths: &[f32], tabs: bool) -> Vec<Rect> {
    let mut left = if tabs { rect.min.x } else { rect.min.x + 3.0 };
    let top = rect.min.y + 3.0;
    let height = rect.height() - 6.0;
    let measured = match content {
        Content::Segment(segment) => {
            if segment.fill && !widths.is_empty() {
                widths
                    .iter()
                    .map(|_| (rect.width() - 6.0) / widths.len() as f32)
                    .collect::<Vec<_>>()
            } else {
                widths.to_vec()
            }
        }
        Content::Tabs(_) => widths.to_vec(),
        _ => Vec::new(),
    };
    measured
        .iter()
        .map(|width| {
            let cell = Rect::from_min_size(Vec2::new(left, top), Vec2::new(*width, height));
            left += width;
            cell
        })
        .collect()
}

fn measure_widths(content: &Content, _rect: Rect, text: &mut crate::text::Text) -> Vec<f32> {
    match content {
        Content::Segment(segment) => {
            let style = TextStyle::new(segment.font, Color::WHITE);
            text_widths(&segment.options, text, style)
        }
        Content::Tabs(tabs) => {
            let style = TextStyle::new(tabs.font, Color::WHITE);
            text_widths(&tabs.options, text, style)
        }
        _ => Vec::new(),
    }
}

pub(crate) fn pill_target(
    segment: &SegmentContent,
    rect: Rect,
    text: &mut crate::text::Text,
    _theme: &Theme,
) -> (f32, f32) {
    let widths = measure_widths(&Content::Segment(segment.clone()), rect, text);
    let cells = cell_widths(&Content::Segment(segment.clone()), rect, &widths, false);
    let target = segment
        .selected
        .min(segment.options.len().saturating_sub(1));
    let cell = cells.get(target).copied().unwrap_or(rect);
    (cell.min.x, cell.width())
}

pub(crate) fn tabs_pill_target(
    tabs: &TabsContent,
    rect: Rect,
    text: &mut crate::text::Text,
    _theme: &Theme,
) -> (f32, f32) {
    let widths = measure_widths(&Content::Tabs(tabs.clone()), rect, text);
    let cells = cell_widths(&Content::Tabs(tabs.clone()), rect, &widths, true);
    let target = tabs.selected.min(tabs.options.len().saturating_sub(1));
    let cell = cells.get(target).copied().unwrap_or(rect);
    (cell.min.x, cell.width())
}

pub(crate) fn hover_cell(
    content: &Content,
    rect: Rect,
    pointer: Vec2,
    text: &mut crate::text::Text,
) -> usize {
    let tabs = matches!(content, Content::Tabs(_));
    let widths = measure_widths(content, rect, text);
    cell_widths(content, rect, &widths, tabs)
        .iter()
        .position(|cell| cell.contains(pointer))
        .unwrap_or(usize::MAX)
}

pub(super) fn paint_segment(
    segment: &SegmentContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
) {
    let rect = info.rect;
    let palette = info.theme.palette;
    let style = TextStyle::new(segment.font, Color::WHITE);
    let widths = segment
        .options
        .iter()
        .map(|option| painter.measure(option, style).x + 24.0)
        .collect::<Vec<_>>();
    let cells = cell_widths(&Content::Segment(segment.clone()), rect, &widths, false);
    painter.rect(
        rect,
        Corners::capsule(),
        palette.material_fill.amplify(0.75),
    );
    for (index, cell) in cells.iter().enumerate() {
        if index == runtime.thumb as usize && info.hover > 0.01 {
            painter.rect(
                *cell,
                Corners::capsule(),
                palette.material_hover.amplify(0.5 * info.hover),
            );
        }
    }
    for (index, cell) in cells.iter().enumerate() {
        let active = index == segment.selected;
        let color = if active {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        painter.text_in(
            *cell,
            &segment.options[index],
            TextStyle::new(style.font, color).aligned(Align::Center),
            Align::Center,
        );
    }
    let pill = Rect::from_min_size(
        Vec2::new(runtime.value.value, rect.min.y + 3.0),
        Vec2::new(runtime.extent.value, rect.height() - 6.0),
    );
    glass.push(SubGlass {
        rect: pill,
        style: GlassStyle::tab_lens(),
        engaged: info.press > 0.5,
    });
}

pub(super) fn paint_tabs(
    tabs: &TabsContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
) {
    let rect = info.rect;
    let palette = info.theme.palette;
    let metrics = info.theme.metrics;
    let style = TextStyle::new(tabs.font, Color::WHITE);
    if tabs.bordered {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(rect.min.x, rect.max.y - metrics.hairline),
                Vec2::new(rect.width(), metrics.hairline),
            ),
            Corners::ZERO,
            palette.separator,
        );
    }
    let widths = tabs
        .options
        .iter()
        .map(|option| painter.measure(option, style).x + 24.0)
        .collect::<Vec<_>>();
    let cells = cell_widths(&Content::Tabs(tabs.clone()), rect, &widths, true);
    for (index, cell) in cells.iter().enumerate() {
        if index == runtime.thumb as usize && info.hover > 0.01 {
            painter.rect(
                cell.shrink2(Vec2::new(2.0, 0.0)),
                Corners::capsule(),
                palette.material_hover.amplify(0.4 * info.hover),
            );
        }
    }
    for (index, cell) in cells.iter().enumerate() {
        let active = index == tabs.selected;
        let color = if active {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        painter.text_in(
            *cell,
            &tabs.options[index],
            TextStyle::new(style.font, color).aligned(Align::Center),
            Align::Center,
        );
    }
    let pill = Rect::from_min_size(
        Vec2::new(runtime.value.value, rect.min.y + 3.0),
        Vec2::new(runtime.extent.value, 26.0),
    );
    glass.push(SubGlass {
        rect: pill,
        style: GlassStyle::tab_lens(),
        engaged: info.press > 0.5,
    });
}
