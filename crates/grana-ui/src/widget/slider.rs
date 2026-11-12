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
        size: Size::new(Length::Fill, Length::Fixed(26.0)),
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
            Length::Fixed(64.0),
            Length::Fixed(theme.metrics.control_height),
        ),
        ..Layout::row()
    })
    .with_content(Content::DragValue(DragValueContent {
        value,
        decimals,
        font: theme.font(Level::Monospace),
        width: 64.0,
    }))
}

pub fn progress<'a, A>(fraction: f32, width: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(width), Length::Fixed(4.0)),
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
        radius: Corners::all(4.0),
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
    Vec2::new(width, 26.0)
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
        let style = TextStyle::new(theme.label_font(), theme.palette.text_tertiary);
        text.layout(label, style.font, 0.0, false, false, Align::Start)
            .size
            .x
            .clamp(58.0, 92.0)
            + 8.0
    });
    let readout_width = if slider.readout.is_some() { 56.0 } else { 0.0 };
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
        Motion::response(0.12, 0.0)
    } else {
        Motion::snappy()
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
        Motion::response(0.12, 0.0)
    } else {
        Motion::snappy()
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
    let caption = TextStyle::new(theme.label_font(), palette.text_tertiary);
    let label_width = slider.label.as_ref().map_or(0.0, |label| {
        painter.measure(label, caption).x.clamp(58.0, 92.0) + 8.0
    });
    let readout_width = if slider.readout.is_some() { 56.0 } else { 0.0 };
    let rect = info.rect;
    let hit = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 12.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 12.0),
    );
    let bar = Rect::from_center_size(hit.center(), Vec2::new(hit.width(), metrics.track));
    let position = runtime.value.value.clamp(0.0, 1.0);
    let engaged = info.engaged;
    let center = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let knob_size = Vec2::new(11.0 + engaged * 1.5, 15.0 + engaged * 1.5);
    let knob_rect = Rect::from_center_size(center, knob_size);
    let fill_end = center.x.clamp(bar.min.x, bar.max.x);
    let filled = Rect::from_min_max(bar.min, Vec2::new(fill_end, bar.max.y));
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
    painter.rect(
        bar,
        Corners::all(2.0),
        palette
            .track
            .with_alpha(palette.track.alpha * (0.85 + 0.3 * engaged)),
    );
    if filled.width() > 0.0 {
        painter.gradient(
            filled,
            Corners::all(2.0),
            palette.accent_hover,
            palette.accent,
        );
    }
    if let Some(readout) = &slider.readout {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - readout_width + 4.0, rect.min.y),
                rect.max,
            ),
            readout,
            TextStyle::new(theme.font(Level::Monospace), palette.text_secondary)
                .aligned(Align::End),
            Align::Center,
        );
    }
    painter.outlined(
        knob_rect,
        Corners::all(3.5),
        Fill::Gradient(Color::bytes(245, 247, 250), Color::bytes(210, 214, 222)),
        Border {
            width: 1.0,
            color: if info.press > 0.5 {
                palette.accent
            } else {
                Color::white_alpha(0.9)
            },
        },
        Shadow::new(Color::black_alpha(0.45), 4.0, 1.0),
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
        Vec2::new(rect.min.x + 6.0, rect.center().y - 4.0),
        Vec2::new(rect.max.x - 6.0, rect.center().y + 4.0),
    );
    let low_t = runtime.value.value.clamp(0.0, 1.0);
    let high_t = runtime.extent.value.clamp(0.0, 1.0);
    let low_x = bar.min.x + bar.width() * low_t;
    let high_x = bar.min.x + bar.width() * high_t;
    let engaged = info.engaged;
    painter.outlined(
        bar,
        Corners::all(3.0),
        Fill::Solid(palette.field),
        Border {
            width: 1.0,
            color: palette.material_border,
        },
        Shadow::NONE,
    );
    if range.gradient.len() > 1 {
        let clip = painter.clip();
        painter.push_clip(bar, Corners::all(3.0));
        let segments = range.gradient.len() - 1;
        for (index, color) in range.gradient.iter().take(segments).enumerate() {
            let left = bar.min.x + bar.width() * index as f32 / segments as f32;
            let right = bar.min.x + bar.width() * (index + 1) as f32 / segments as f32;
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(left, bar.min.y),
                    Vec2::new(right + 0.6, bar.max.y),
                ),
                Corners::ZERO,
                *color,
            );
        }
        painter.pop_clip(clip);
    }
    let shade = palette.shade.amplify(0.65);
    painter.rect(
        Rect::from_min_max(bar.min, Vec2::new(low_x, bar.max.y)),
        Corners {
            top_left: 3.0,
            bottom_left: 3.0,
            ..Corners::ZERO
        },
        shade,
    );
    painter.rect(
        Rect::from_min_max(Vec2::new(high_x, bar.min.y), bar.max),
        Corners {
            top_right: 3.0,
            bottom_right: 3.0,
            ..Corners::ZERO
        },
        shade,
    );
    let handle_size = Vec2::new(8.0 + engaged * 1.5, 16.0 + engaged * 2.0);
    for x in [low_x, high_x] {
        let knob = Rect::from_center_size(Vec2::new(x, bar.center().y), handle_size);
        painter.outlined(
            knob,
            Corners::all(2.5),
            Fill::Gradient(Color::bytes(245, 247, 250), Color::bytes(210, 214, 222)),
            Border {
                width: 1.0,
                color: Color::white_alpha(0.9),
            },
            Shadow::new(Color::black_alpha(0.45), 3.0, 1.0),
        );
    }
}

pub(super) fn paint_drag(drag: &DragValueContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let look = info.theme.surface(Role::Field, crate::style::State::Rest);
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
    painter.rect(rect, Corners::all(2.0), palette.track);
    let filled = rect.width() * progress.fraction.clamp(0.0, 1.0);
    if filled > 1.0 {
        painter.gradient(
            Rect::from_min_size(rect.min, Vec2::new(filled, rect.height())),
            Corners::all(2.0),
            palette.accent_hover,
            palette.accent,
        );
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
            color: info.theme.palette.material_border,
        },
        Shadow::NONE,
    );
}
