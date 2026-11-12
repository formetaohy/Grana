use std::ops::RangeInclusive;

use glam::Vec2;

use crate::color::Color;
use crate::content::{
    Content, DragValueContent, GradientBarContent, ProgressContent, RangeSliderContent,
    SliderContent,
};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role, Theme};

use super::PaintInfo;

const ARROW_STEP: f32 = 0.01;
const PAGE_STEP: f32 = 0.1;

pub fn slider_sized<'a, A>(
    theme: &Theme,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    width: Length,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(width, Length::Fixed(theme.metrics.control_height)),
        ..Layout::row()
    })
    .with_content(Content::Slider(SliderContent {
        value,
        range,
        label: label.map(str::to_string),
        readout: None,
        logarithmic,
        width,
    }))
}

pub fn slider_readout<'a, A>(
    theme: &Theme,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    readout: &str,
) -> Element<'a, A> {
    let mut element = slider_sized(theme, label, value, range, logarithmic, Length::Fill);
    if let Content::Slider(slider) = &mut element.content {
        slider.readout = Some(readout.to_string());
    }
    element
}

pub fn range_slider<'a, A>(
    start: f32,
    end: f32,
    range: RangeInclusive<f32>,
    gradient: &[Color],
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(28.0)),
        ..Layout::row()
    })
    .with_content(Content::RangeSlider(RangeSliderContent {
        start,
        end,
        range,
        gradient: gradient.to_vec(),
        width: Length::Fill,
    }))
}

pub fn drag_value<'a, A>(theme: &Theme, value: f32, decimals: usize) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(
            Length::Fixed(68.0),
            Length::Fixed(theme.metrics.control_height),
        ),
        ..Layout::row()
    })
    .with_content(Content::DragValue(DragValueContent {
        value,
        decimals,
        font: theme.font(Level::Monospace),
        width: 68.0,
    }))
}

pub fn progress<'a, A>(fraction: f32, width: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(width), Length::Fixed(10.0)),
        ..Layout::row()
    })
    .with_content(Content::Progress(ProgressContent { fraction, width }))
}

pub fn gradient_bar<'a, A>(colors: &[Color], height: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(height)),
        ..Layout::row()
    })
    .with_content(Content::GradientBar(GradientBarContent {
        colors: colors.to_vec(),
        height,
        radius: Corners::all(height * 0.5),
    }))
}

pub(super) fn measure_slider(slider: &SliderContent, available: Vec2, theme: &Theme) -> Vec2 {
    let width = match slider.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => maximum.min(available.x),
    };
    Vec2::new(width, theme.metrics.control_height)
}

pub(super) fn measure_range(range: &RangeSliderContent, available: Vec2) -> Vec2 {
    let width = match range.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => maximum.min(available.x),
    };
    Vec2::new(width, 28.0)
}

pub(super) fn measure_drag(drag: &DragValueContent, theme: &Theme) -> Vec2 {
    Vec2::new(drag.width, theme.metrics.control_height)
}

pub(crate) fn track(
    slider: &SliderContent,
    rect: Rect,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Rect {
    let label_width = slider.label.as_ref().map_or(0.0, |label| {
        let style = TextStyle::new(theme.label_font(), theme.palette.on_surface_variant);
        text.layout(label, style.font, 0.0, false, false, Align::Start)
            .size
            .x
            .clamp(58.0, 92.0)
            + 8.0
    });
    let readout_width = if slider.readout.is_some() { 58.0 } else { 0.0 };
    Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 12.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 12.0),
    )
}

pub(crate) fn normalized_value(slider: &SliderContent, value: f32) -> f32 {
    let (minimum, maximum) = (*slider.range.start(), *slider.range.end());
    let value = value.clamp(minimum, maximum);
    if slider.logarithmic && minimum > 0.0 {
        ((value.max(minimum).ln() - minimum.ln()) / (maximum.ln() - minimum.ln())).clamp(0.0, 1.0)
    } else {
        ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
    }
}

pub(crate) fn normalized(slider: &SliderContent) -> f32 {
    normalized_value(slider, slider.value)
}

pub(crate) fn range_normalized(range: &RangeSliderContent) -> (f32, f32) {
    let (minimum, maximum) = (*range.range.start(), *range.range.end());
    let span = (maximum - minimum).max(1.0e-6);
    (
        ((range.start - minimum) / span).clamp(0.0, 1.0),
        ((range.end - minimum) / span).clamp(0.0, 1.0),
    )
}

fn absolute(t: f32, range: &RangeInclusive<f32>, logarithmic: bool) -> f32 {
    let (minimum, maximum) = (*range.start(), *range.end());
    let t = t.clamp(0.0, 1.0);
    if logarithmic && minimum > 0.0 {
        (minimum.ln() + t * (maximum.ln() - minimum.ln())).exp()
    } else {
        minimum + t * (maximum - minimum)
    }
}

pub(crate) fn value_at(
    slider: &SliderContent,
    rect: Rect,
    pointer: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> f32 {
    let track = track(slider, rect, text, theme);
    let t = (pointer.x - track.min.x) / track.width().max(1.0);
    absolute(t, &slider.range, slider.logarithmic)
}

pub(crate) fn range_at(range: &RangeSliderContent, rect: Rect, pointer: Vec2) -> (f32, f32) {
    let (minimum, maximum) = (*range.range.start(), *range.range.end());
    let span = (maximum - minimum).max(1.0e-6);
    let track = Rect::from_min_max(
        Vec2::new(rect.min.x + 6.0, rect.min.y),
        Vec2::new(rect.max.x - 6.0, rect.max.y),
    );
    let value =
        minimum + ((pointer.x - track.min.x) / track.width().max(1.0)).clamp(0.0, 1.0) * span;
    if (value - range.start).abs() <= (value - range.end).abs() {
        (value.min(range.end - 1.0e-4), range.end)
    } else {
        (range.start, value.max(range.start + 1.0e-4))
    }
}

pub(crate) fn step(slider: &SliderContent, direction: f32, page: bool) -> f32 {
    let step = if page { PAGE_STEP } else { ARROW_STEP };
    let t = (normalized(slider) + direction * step).clamp(0.0, 1.0);
    absolute(t, &slider.range, slider.logarithmic)
}

pub(crate) fn range_step(range: &RangeSliderContent, direction: f32, page: bool) -> (f32, f32) {
    let (minimum, maximum) = (*range.range.start(), *range.range.end());
    let span = (maximum - minimum).max(1.0e-6);
    let step = span * if page { PAGE_STEP } else { ARROW_STEP };
    let epsilon = 1.0e-4 * span.max(1.0);
    if direction < 0.0 {
        let start = (range.start - step).max(minimum);
        if start < range.start {
            (start, range.end)
        } else {
            (range.start, (range.end - step).max(range.start + epsilon))
        }
    } else {
        let end = (range.end + step).min(maximum);
        if end > range.end {
            (range.start, end)
        } else {
            ((range.start + step).min(range.end - epsilon), range.end)
        }
    }
}

pub(crate) fn drag_step(drag: &DragValueContent, direction: f32, page: bool) -> f32 {
    let scale = 10f32.powi(-(drag.decimals as i32));
    let multiplier = if page { 10.0 } else { 1.0 };
    drag.value + direction * scale * multiplier
}

pub(crate) fn value_slider<A>(node: &Node<A>, pressed: bool) -> Option<(Motion, (f32, f32))> {
    let Content::Slider(slider) = &node.content else {
        return None;
    };
    let motion = if pressed {
        Motion::response(0.10, 0.0)
    } else {
        Motion::spatial_fast()
    };
    let target = if pressed {
        node.runtime.drag_value
    } else {
        normalized(slider)
    };
    Some((motion, (target, 0.0)))
}

pub(crate) fn value_range<A>(node: &Node<A>, pressed: bool) -> Option<(Motion, (f32, f32))> {
    let Content::RangeSlider(range) = &node.content else {
        return None;
    };
    let motion = if pressed {
        Motion::response(0.10, 0.0)
    } else {
        Motion::spatial_fast()
    };
    let target = if pressed {
        (node.runtime.drag_value, node.runtime.drag_extent)
    } else {
        range_normalized(range)
    };
    Some((motion, target))
}

pub(super) fn paint_slider(
    slider: &SliderContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let theme = info.theme;
    let palette = theme.palette;
    let metrics = theme.metrics;
    let caption = TextStyle::new(theme.label_font(), palette.on_surface_variant);
    let label_width = slider.label.as_ref().map_or(0.0, |label| {
        painter.measure(label, caption).x.clamp(58.0, 92.0) + 8.0
    });
    let readout_width = if slider.readout.is_some() { 58.0 } else { 0.0 };
    let rect = info.rect;
    let hit = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 12.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 12.0),
    );
    let track_h = metrics.slider_track;
    let bar = Rect::from_center_size(hit.center(), Vec2::new(hit.width(), track_h));
    let position = runtime.value.value.clamp(0.0, 1.0);
    let engaged = info.engaged;
    let center = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let handle_w = metrics.slider_handle_width + engaged * 1.2;
    let handle_h = metrics.slider_handle_height + engaged * 2.5;
    let handle_rect = Rect::from_center_size(center, Vec2::new(handle_w, handle_h));
    let gap = metrics.slider_gap;
    let outer_r = track_h * 0.5;
    let inner_r = 2.0;
    if let Some(label) = &slider.label {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x, rect.min.y),
                Vec2::new(rect.min.x + label_width - 6.0, rect.max.y),
            ),
            label,
            TextStyle::new(caption.font, caption.color).truncating(),
            Align::Center,
        );
    }
    let active_end = (center.x - gap).clamp(bar.min.x, bar.max.x);
    if active_end > bar.min.x + 1.0 {
        let active_rect = Rect::from_min_max(bar.min, Vec2::new(active_end, bar.max.y));
        painter.rect(
            active_rect,
            Corners::left(outer_r, inner_r),
            palette.primary,
        );
    }
    let inactive_start = (center.x + gap).clamp(bar.min.x, bar.max.x);
    if bar.max.x > inactive_start + 1.0 {
        let inactive_rect = Rect::from_min_max(Vec2::new(inactive_start, bar.min.y), bar.max);
        painter.rect(
            inactive_rect,
            Corners::right(outer_r, inner_r),
            palette.surface_container_highest,
        );
        if inactive_rect.width() > 8.0 {
            let stop = Rect::from_center_size(
                Vec2::new(bar.max.x - 5.0, bar.center().y),
                Vec2::splat(4.0),
            );
            painter.rect(stop, Corners::capsule(), palette.primary);
        }
    }
    if let Some(readout) = &slider.readout {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - readout_width + 4.0, rect.min.y),
                rect.max,
            ),
            readout,
            TextStyle::new(theme.font(Level::Monospace), palette.on_surface).aligned(Align::End),
            Align::Center,
        );
    }
    painter.rect(
        handle_rect,
        Corners::capsule(),
        if info.press > 0.5 {
            palette.primary.lerp(Color::WHITE, 0.25)
        } else {
            palette.primary
        },
    );
}

pub(super) fn paint_range(
    range: &RangeSliderContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let rect = info.rect;
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 4.0, rect.center().y - 8.0),
        Vec2::new(rect.max.x - 4.0, rect.center().y + 8.0),
    );
    let low_t = runtime.value.value.clamp(0.0, 1.0);
    let high_t = runtime.extent.value.clamp(0.0, 1.0);
    let low_x = bar.min.x + bar.width() * low_t;
    let high_x = bar.min.x + bar.width() * high_t;
    let engaged = info.engaged;
    let gap = info.theme.metrics.slider_gap;
    let outer_r = 8.0;
    let inner_r = 2.0;
    let left_end = (low_x - gap).clamp(bar.min.x, bar.max.x);
    if left_end > bar.min.x + 1.0 {
        painter.rect(
            Rect::from_min_max(bar.min, Vec2::new(left_end, bar.max.y)),
            Corners::left(outer_r, inner_r),
            palette.surface_container_highest,
        );
    }
    let right_start = (high_x + gap).clamp(bar.min.x, bar.max.x);
    if bar.max.x > right_start + 1.0 {
        painter.rect(
            Rect::from_min_max(Vec2::new(right_start, bar.min.y), bar.max),
            Corners::right(outer_r, inner_r),
            palette.surface_container_highest,
        );
    }
    let active_min = (low_x + gap).clamp(bar.min.x, bar.max.x);
    let active_max = (high_x - gap).clamp(active_min, bar.max.x);
    if active_max > active_min + 1.0 {
        let active_bar = Rect::from_min_max(
            Vec2::new(active_min, bar.min.y),
            Vec2::new(active_max, bar.max.y),
        );
        if range.gradient.len() > 1 {
            let clip = painter.clip();
            painter.push_clip(active_bar, Corners::all(inner_r));
            let segments = range.gradient.len() - 1;
            for (index, color) in range.gradient.iter().take(segments).enumerate() {
                let left = active_bar.min.x + active_bar.width() * index as f32 / segments as f32;
                let right =
                    active_bar.min.x + active_bar.width() * (index + 1) as f32 / segments as f32;
                painter.rect(
                    Rect::from_min_max(
                        Vec2::new(left, active_bar.min.y),
                        Vec2::new(right + 0.6, active_bar.max.y),
                    ),
                    Corners::ZERO,
                    *color,
                );
            }
            painter.pop_clip(clip);
        } else {
            painter.rect(active_bar, Corners::all(inner_r), palette.primary);
        }
    }
    let handle_color = range
        .gradient
        .last()
        .copied()
        .unwrap_or(palette.primary)
        .lerp(Color::WHITE, 0.18);
    let handle_size = Vec2::new(4.0 + engaged * 1.2, 24.0 + engaged * 2.0);
    for x in [low_x, high_x] {
        let handle = Rect::from_center_size(Vec2::new(x, bar.center().y), handle_size);
        painter.rect(handle, Corners::capsule(), handle_color);
    }
}

pub(super) fn paint_drag(drag: &DragValueContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let state = if info.press > 0.5 {
        crate::style::State::Pressed
    } else if info.hover > 0.5 {
        crate::style::State::Hover
    } else {
        crate::style::State::Rest
    };
    let look = info.theme.surface(Role::Field, state);
    painter.surface(info.rect, look);
    let text = format!("{:.*}", drag.decimals, drag.value);
    painter.text_in(
        info.rect.shrink2(Vec2::new(8.0, 0.0)),
        &text,
        TextStyle::new(drag.font, look.text).aligned(Align::End),
        Align::Center,
    );
}

pub(super) fn paint_progress(
    progress: &ProgressContent,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let rect = info.rect;
    let center_y = rect.center().y;
    let fraction = progress.fraction.clamp(0.0, 1.0);
    let filled = rect.width() * fraction;
    let gap = 4.0;
    let active_end = (rect.min.x + filled - gap * 0.5).clamp(rect.min.x, rect.max.x);
    if active_end > rect.min.x + 2.0 {
        let step = 3.0;
        let phase = info.time * 6.5;
        let wavelength = 16.0;
        let amplitude = 2.0;
        let mut x = rect.min.x + 1.5;
        while x < active_end {
            let next_x = (x + step).min(active_end);
            let y0 = center_y
                + amplitude
                    * (((x - rect.min.x) / wavelength) * std::f32::consts::TAU - phase).sin();
            let y1 = center_y
                + amplitude
                    * (((next_x - rect.min.x) / wavelength) * std::f32::consts::TAU - phase).sin();
            painter.segment(
                Vec2::new(x, y0),
                Vec2::new(next_x, y1),
                2.8,
                palette.tertiary,
            );
            x = next_x;
        }
    }
    let inactive_start = (rect.min.x + filled + gap * 0.5).clamp(rect.min.x, rect.max.x);
    if rect.max.x > inactive_start + 2.0 {
        let bar = Rect::from_min_max(
            Vec2::new(inactive_start, center_y - 2.0),
            Vec2::new(rect.max.x, center_y + 2.0),
        );
        painter.rect(bar, Corners::capsule(), palette.surface_container_highest);
        let dot = Rect::from_center_size(Vec2::new(rect.max.x - 2.0, center_y), Vec2::splat(4.0));
        painter.rect(dot, Corners::capsule(), palette.tertiary);
    }
}

pub(super) fn paint_gradient_bar(
    bar: &GradientBarContent,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    if bar.colors.len() < 2 {
        return;
    }
    let rect = info.rect;
    let clip = painter.clip();
    painter.push_clip(rect, bar.radius);
    let segments = bar.colors.len() - 1;
    for (index, color) in bar.colors.iter().take(segments).enumerate() {
        let left = rect.min.x + rect.width() * index as f32 / segments as f32;
        let right = rect.min.x + rect.width() * (index + 1) as f32 / segments as f32;
        painter.rect(
            Rect::from_min_max(
                Vec2::new(left, rect.min.y),
                Vec2::new(right + 0.6, rect.max.y),
            ),
            Corners::ZERO,
            *color,
        );
    }
    painter.pop_clip(clip);
    painter.outlined(
        rect,
        bar.radius,
        Fill::None,
        Border {
            width: 1.0,
            color: info.theme.palette.outline_variant.with_alpha(0.5),
        },
        Shadow::NONE,
    );
}
