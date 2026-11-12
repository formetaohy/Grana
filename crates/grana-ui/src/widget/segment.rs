use glam::Vec2;

use crate::color::Color;
use crate::content::{Content, SegmentContent, TabsContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Theme};

use super::PaintInfo;

const TABS_HEIGHT: f32 = 32.0;
const CELL_PAD: f32 = 22.0;
const GROUP_GAP: f32 = 2.0;

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
    let width = if fill { Length::Fill } else { Length::Auto };
    Element::new(Layout {
        size: Size::new(width, Length::Fixed(theme.metrics.control_height)),
        ..Layout::row()
    })
    .with_content(Content::Segment(SegmentContent {
        options: options.to_vec(),
        selected,
        fill,
        font: theme.font(Level::Label),
        width,
    }))
}

pub fn tabs<'a, A>(theme: &Theme, options: &[String], selected: usize) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(TABS_HEIGHT)),
        ..Layout::row()
    })
    .with_content(Content::Tabs(TabsContent {
        options: options.to_vec(),
        selected,
        font: theme.font(Level::Label),
    }))
}

pub(super) fn measure_segment(
    segment: &SegmentContent,
    available: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Vec2 {
    let count = segment.options.len();
    let gaps = if count > 1 {
        GROUP_GAP * (count - 1) as f32
    } else {
        0.0
    };
    let total = if segment.fill {
        available.x
    } else {
        widths(&segment.options, segment.font, text)
            .iter()
            .sum::<f32>()
            + gaps
    };
    let _ = &segment.width;
    Vec2::new(total, theme.metrics.control_height)
}

pub(super) fn measure_tabs(tabs: &TabsContent, available: Vec2) -> Vec2 {
    let _ = tabs;
    Vec2::new(available.x, TABS_HEIGHT)
}

fn widths(
    options: &[String],
    font: crate::theme::TypeStyle,
    text: &mut crate::text::Text,
) -> Vec<f32> {
    let style = TextStyle::new(font, Color::WHITE);
    options
        .iter()
        .map(|option| {
            text.layout(option, style.font, 0.0, false, false, Align::Start)
                .size
                .x
                + CELL_PAD
        })
        .collect()
}

fn cells(rect: Rect, widths: &[f32], fill: bool, tabs: bool) -> Vec<Rect> {
    if widths.is_empty() {
        return Vec::new();
    }
    let gap = if tabs { 2.0 } else { GROUP_GAP };
    let inset = if tabs { 3.0 } else { 0.0 };
    let mut left = rect.min.x + inset;
    let top = rect.min.y + inset;
    let height = (rect.height() - inset * 2.0).max(1.0);
    let total_gaps = gap * (widths.len().saturating_sub(1)) as f32;
    let usable = (rect.width() - inset * 2.0 - total_gaps).max(0.0);
    let resolved = if fill {
        vec![usable / widths.len() as f32; widths.len()]
    } else {
        let sum = widths.iter().sum::<f32>().max(1.0);
        if sum > usable && usable > 0.0 {
            let scale = usable / sum;
            widths.iter().map(|w| w * scale).collect()
        } else {
            widths.to_vec()
        }
    };
    resolved
        .into_iter()
        .map(|width| {
            let cell = Rect::from_min_size(Vec2::new(left, top), Vec2::new(width, height));
            left += width + gap;
            cell
        })
        .collect()
}

fn cell_widths(content: &Content, rect: Rect, widths: &[f32]) -> Vec<Rect> {
    match content {
        Content::Segment(segment) => cells(rect, widths, segment.fill, false),
        Content::Tabs(_) => cells(rect, widths, false, true),
        _ => Vec::new(),
    }
}

fn measure_widths(content: &Content, text: &mut crate::text::Text) -> Vec<f32> {
    match content {
        Content::Segment(segment) => widths(&segment.options, segment.font, text),
        Content::Tabs(tabs) => widths(&tabs.options, tabs.font, text),
        _ => Vec::new(),
    }
}

pub(crate) fn selected(content: &Content) -> usize {
    match content {
        Content::Segment(segment) => segment
            .selected
            .min(segment.options.len().saturating_sub(1)),
        Content::Tabs(tabs) => tabs.selected.min(tabs.options.len().saturating_sub(1)),
        _ => 0,
    }
}

pub(crate) fn options_len(content: &Content) -> usize {
    match content {
        Content::Segment(segment) => segment.options.len(),
        Content::Tabs(tabs) => tabs.options.len(),
        _ => 0,
    }
}

pub(crate) fn step_index(content: &Content, direction: isize) -> usize {
    let count = options_len(content);
    if count == 0 {
        return 0;
    }
    (selected(content) as isize + direction).clamp(0, count as isize - 1) as usize
}

pub(crate) fn cell_at(
    content: &Content,
    rect: Rect,
    pointer: Vec2,
    text: &mut crate::text::Text,
) -> Option<usize> {
    let widths = measure_widths(content, text);
    cell_widths(content, rect, &widths)
        .iter()
        .position(|cell| cell.contains(pointer))
}

pub(crate) fn pill_target(
    content: &Content,
    rect: Rect,
    text: &mut crate::text::Text,
) -> (f32, f32) {
    let widths = measure_widths(content, text);
    let cells = cell_widths(content, rect, &widths);
    let cell = cells.get(selected(content)).copied().unwrap_or(rect);
    (cell.min.x, cell.width())
}

pub(crate) fn cell_target(
    content: &Content,
    rect: Rect,
    text: &mut crate::text::Text,
    index: usize,
) -> (f32, f32) {
    let widths = measure_widths(content, text);
    let cells = cell_widths(content, rect, &widths);
    let cell = cells.get(index).copied().unwrap_or(rect);
    (cell.min.x, cell.width())
}

pub(crate) fn value_segment<A>(
    node: &Node<A>,
    dragging: bool,
    pointer: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Option<(Motion, (f32, f32))> {
    let Content::Segment(_) = &node.content else {
        return None;
    };
    let motion = if dragging {
        Motion::INSTANT
    } else if theme.reduce_motion {
        Motion::effect_default()
    } else {
        Motion::spatial_fast()
    };
    let target = if dragging {
        drag_pill(&node.content, node.rect, pointer, text)
    } else if let Some(commit) = node.runtime.commit {
        cell_target(&node.content, node.rect, text, commit.round() as usize)
    } else {
        pill_target(&node.content, node.rect, text)
    };
    Some((motion, target))
}

pub(crate) fn value_tabs<A>(
    node: &Node<A>,
    dragging: bool,
    pointer: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Option<(Motion, (f32, f32))> {
    let Content::Tabs(_) = &node.content else {
        return None;
    };
    let motion = if dragging {
        Motion::INSTANT
    } else if theme.reduce_motion {
        Motion::effect_default()
    } else {
        Motion::spatial_fast()
    };
    let target = if dragging {
        drag_pill(&node.content, node.rect, pointer, text)
    } else if let Some(commit) = node.runtime.commit {
        cell_target(&node.content, node.rect, text, commit.round() as usize)
    } else {
        pill_target(&node.content, node.rect, text)
    };
    Some((motion, target))
}

fn drag_pill(
    content: &Content,
    rect: Rect,
    pointer: Vec2,
    text: &mut crate::text::Text,
) -> (f32, f32) {
    let widths = measure_widths(content, text);
    let cells = cell_widths(content, rect, &widths);
    let Some(first) = cells.first().copied() else {
        return (rect.min.x, 0.0);
    };
    let last = cells.last().copied().unwrap_or(first);
    let width = drag_width(&cells, pointer.x);
    let left = (pointer.x - width * 0.5).clamp(first.min.x, (last.max.x - width).max(first.min.x));
    (left, width)
}

fn drag_width(cells: &[Rect], x: f32) -> f32 {
    let Some(first) = cells.first() else {
        return 0.0;
    };
    if x <= first.center().x {
        return first.width();
    }
    for pair in cells.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if x <= right.center().x {
            let span = (right.center().x - left.center().x).max(1.0);
            let t = ((x - left.center().x) / span).clamp(0.0, 1.0);
            return left.width() + (right.width() - left.width()) * t;
        }
    }
    cells.last().map_or(first.width(), |cell| cell.width())
}

pub(super) fn paint_segment(
    segment: &SegmentContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let rect = info.rect;
    let palette = info.theme.palette;
    let metrics = info.theme.metrics;
    let style = TextStyle::new(segment.font, Color::WHITE);
    let widths = segment
        .options
        .iter()
        .map(|option| painter.measure(option, style).x + CELL_PAD)
        .collect::<Vec<_>>();
    let cells = cells(rect, &widths, segment.fill, false);
    let count = cells.len();
    let hovered_index = runtime.thumb as usize;
    for (index, cell) in cells.iter().enumerate() {
        let active = index == segment.selected;
        let cell_hovered = index == hovered_index && info.hover > 0.01;
        let cell_pressed = cell_hovered && info.press > 0.01;
        let base_corners =
            Corners::group_horizontal(index, count, Corners::CAPSULE, metrics.corner_sm, active);
        let corners = if cell_pressed {
            base_corners
                .resolved(cell.size())
                .lerp(Corners::all(metrics.corner_xs), info.press)
        } else {
            base_corners
        };
        if !active {
            let fill = if cell_pressed {
                palette
                    .surface_container_highest
                    .lerp(palette.on_surface, 0.08)
            } else if cell_hovered {
                palette
                    .surface_container_high
                    .lerp(palette.surface_container_highest, info.hover)
            } else {
                palette.surface_container_high
            };
            painter.rect(*cell, corners, fill);
        }
    }
    if !cells.is_empty() {
        let pill = Rect::from_min_size(
            Vec2::new(runtime.value.value, rect.min.y),
            Vec2::new(runtime.extent.value.max(1.0), rect.height()),
        );
        let active_hovered = segment.selected == hovered_index && info.hover > 0.01;
        let pill_fill = if active_hovered && info.press > 0.01 {
            palette
                .secondary_container
                .lerp(palette.on_secondary_container, 0.14)
        } else if active_hovered {
            palette
                .secondary_container
                .lerp(palette.on_secondary_container, 0.08 * info.hover)
        } else {
            palette.secondary_container
        };
        painter.rect(pill, Corners::capsule(), pill_fill);
    }
    for (index, cell) in cells.iter().enumerate() {
        let active = index == segment.selected;
        let cell_hovered = index == hovered_index && info.hover > 0.01;
        let color = if active {
            palette.on_secondary_container
        } else if cell_hovered {
            palette.on_surface
        } else {
            palette.on_surface_variant
        };
        if let Some(option) = segment.options.get(index) {
            painter.text_in(
                *cell,
                option,
                TextStyle::new(style.font, color)
                    .aligned(Align::Center)
                    .truncating(),
                Align::Center,
            );
        }
    }
}

pub(super) fn paint_tabs(
    tabs: &TabsContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let rect = info.rect;
    let palette = info.theme.palette;
    let style = TextStyle::new(tabs.font, Color::WHITE);
    let widths = tabs
        .options
        .iter()
        .map(|option| painter.measure(option, style).x + CELL_PAD)
        .collect::<Vec<_>>();
    let cells = cells(rect, &widths, false, true);
    if let Some(last) = cells.last() {
        let bar = Rect::from_min_max(rect.min, Vec2::new(last.max.x + 3.0, rect.max.y));
        painter.rect(bar, Corners::capsule(), palette.surface_container);
    }
    for (index, cell) in cells.iter().enumerate() {
        if index != tabs.selected
            && index == runtime.thumb as usize
            && info.hover > 0.01
            && info.press < 0.5
        {
            painter.rect(
                *cell,
                Corners::capsule(),
                palette.on_surface.with_alpha(0.08 * info.hover),
            );
        }
    }
    let pill = Rect::from_min_size(
        Vec2::new(runtime.value.value, rect.min.y + 3.0),
        Vec2::new(
            runtime.extent.value.max(1.0),
            (rect.height() - 6.0).max(1.0),
        ),
    );
    painter.rect(pill, Corners::capsule(), palette.secondary_container);
    for (index, cell) in cells.iter().enumerate() {
        let active = index == tabs.selected;
        let color = if active {
            palette.on_secondary_container
        } else {
            palette.on_surface_variant
        };
        if let Some(option) = tabs.options.get(index) {
            painter.text_in(
                *cell,
                option,
                TextStyle::new(style.font, color).aligned(Align::Center),
                Align::Center,
            );
        }
    }
}
