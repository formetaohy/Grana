use std::ops::RangeInclusive;

use glam::Vec2;

use crate::color::Color;
use crate::element::{
    Content, DragValueContent, Element, GradientBarContent, Layout, Length, ProgressContent,
    RangeSliderContent, Size, SliderContent,
};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::node::Runtime;
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role, Theme};

use super::{PaintInfo, SubGlass};

pub fn slider<'a, A>(
    theme: &Theme,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
) -> Element<'a, A> {
    slider_sized(theme, label, value, range, logarithmic, Length::Fill)
}

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
    .focus_ring(Corners::all(theme.metrics.radius))
    .cursor(crate::input::Cursor::ResizeHorizontal)
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
    theme: &Theme,
    start: f32,
    end: f32,
    range: RangeInclusive<f32>,
    gradient: &[Color],
) -> Element<'a, A> {
    let _ = theme;
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(32.0)),
        ..Layout::row()
    })
    .with_content(Content::RangeSlider(RangeSliderContent {
        start,
        end,
        range,
        gradient: gradient.to_vec(),
        width: Length::Fill,
    }))
    .cursor(crate::input::Cursor::ResizeHorizontal)
    .focus_ring(Corners::capsule())
}

pub fn drag_value<'a, A>(theme: &Theme, value: f32, decimals: usize) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(
            Length::Fixed(66.0),
            Length::Fixed(theme.metrics.control_height),
        ),
        ..Layout::row()
    })
    .with_content(Content::DragValue(DragValueContent {
        value,
        decimals,
        font: theme.font(Level::Monospace),
        width: 66.0,
    }))
    .cursor(crate::input::Cursor::ResizeHorizontal)
    .focus_ring(Corners::all(10.0))
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
        radius: Corners::capsule(),
    }))
}

pub(super) fn measure_slider(
    slider: &SliderContent,
    available: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Vec2 {
    let width = match slider.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => maximum.min(available.x),
    };
    let _ = (text, theme);
    Vec2::new(width, theme.metrics.control_height)
}

pub(super) fn measure_range(range: &RangeSliderContent, available: Vec2) -> Vec2 {
    let width = match range.width {
        Length::Auto | Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => minimum.max(available.x),
        Length::Maximum(maximum) => maximum.min(available.x),
    };
    Vec2::new(width, 32.0)
}

pub(super) fn measure_drag(drag: &DragValueContent, theme: &Theme) -> Vec2 {
    Vec2::new(drag.width, theme.metrics.control_height)
}

pub(crate) fn track_rect(
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
            .clamp(62.0, 96.0)
            + 10.0
    });
    let readout_width = if slider.readout.is_some() { 58.0 } else { 0.0 };
    Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 13.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 13.0),
    )
}

pub(super) fn paint_slider(
    slider: &SliderContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
) {
    let theme = info.theme;
    let palette = theme.palette;
    let metrics = theme.metrics;
    let caption = TextStyle::new(theme.label_font(), palette.text_tertiary);
    let label_width = slider.label.as_ref().map_or(0.0, |label| {
        painter.measure(label, caption).x.clamp(62.0, 96.0) + 10.0
    });
    let readout_width = if slider.readout.is_some() { 58.0 } else { 0.0 };
    let rect = info.rect;
    let hit = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 13.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 13.0),
    );
    let bar = Rect::from_center_size(hit.center(), Vec2::new(hit.width(), metrics.track));
    let knob = metrics.thumb + 4.0;
    let knob_height = 17.0;
    let travel = (bar.width() - knob).max(0.0);
    let position = runtime.value.value.clamp(0.0, 1.0);
    let engaged = info.engaged;
    let size_w = knob + engaged * 2.0;
    let size_h = knob_height + engaged * 1.5;
    let center = Vec2::new(bar.min.x + knob * 0.5 + travel * position, bar.center().y);
    let lag = (runtime.value.target.clamp(0.0, 1.0) - position) * travel;
    let reach = (lag.abs() * 0.48).min(knob * 0.45);
    let (left, right) = if lag >= 0.0 {
        (center.x - size_w * 0.5, center.x + size_w * 0.5 + reach)
    } else {
        (center.x - size_w * 0.5 - reach, center.x + size_w * 0.5)
    };
    let knob_rect = Rect::from_min_max(
        Vec2::new(left, center.y - size_h * 0.5),
        Vec2::new(right, center.y + size_h * 0.5),
    );
    let fill_end = (knob_rect.min.x + position * knob_rect.width()).clamp(bar.min.x, bar.max.x);
    let filled = Rect::from_min_max(bar.min, Vec2::new(fill_end, bar.max.y));
    if let Some(label) = &slider.label {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x, rect.min.y),
                Vec2::new(rect.min.x + label_width - 8.0, rect.max.y),
            ),
            label,
            TextStyle::new(caption.font, caption.color).truncating(),
            Align::Center,
        );
    }
    painter.rect(
        bar,
        Corners::capsule(),
        palette
            .track
            .with_alpha(palette.track.alpha * (0.85 + 0.3 * engaged)),
    );
    if filled.width() > 0.0 {
        painter.gradient(
            filled,
            Corners::capsule(),
            palette.accent_hover,
            palette.accent,
        );
    }
    if let Some(readout) = &slider.readout {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - readout_width + 6.0, rect.min.y),
                rect.max,
            ),
            readout,
            TextStyle::new(theme.font(Level::Monospace), palette.text_secondary)
                .aligned(Align::End),
            Align::Center,
        );
    }
    glass.push(SubGlass {
        rect: knob_rect,
        style: GlassStyle::knob(),
        engaged: info.press > 0.5,
    });
}

pub(super) fn paint_range(
    range: &RangeSliderContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
) {
    let palette = info.theme.palette;
    let rect = info.rect;
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 8.0, rect.center().y - 4.0),
        Vec2::new(rect.max.x - 8.0, rect.center().y + 4.0),
    );
    let low_t = runtime.value.value.clamp(0.0, 1.0);
    let high_t = runtime.extent.value.clamp(0.0, 1.0);
    let low_x = bar.min.x + bar.width() * low_t;
    let high_x = bar.min.x + bar.width() * high_t;
    let engaged = info.engaged;
    painter.outlined(
        bar,
        Corners::capsule(),
        Fill::Solid(palette.track),
        Border::NONE,
        Shadow::NONE,
    );
    if range.gradient.len() > 1 {
        let clip = painter.clip();
        painter.push_clip(bar, Corners::capsule());
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
    let shade = palette.shade.amplify(0.55);
    painter.rect(
        Rect::from_min_max(bar.min, Vec2::new(low_x, bar.max.y)),
        Corners {
            top_left: 4.0,
            bottom_left: 4.0,
            ..Corners::ZERO
        },
        shade,
    );
    painter.rect(
        Rect::from_min_max(Vec2::new(high_x, bar.min.y), bar.max),
        Corners {
            top_right: 4.0,
            bottom_right: 4.0,
            ..Corners::ZERO
        },
        shade,
    );
    let knob_size = Vec2::new(12.0 + engaged * 2.0, 20.0 + engaged * 3.0);
    for x in [low_x, high_x] {
        let knob = Rect::from_center_size(Vec2::new(x, bar.center().y), knob_size);
        glass.push(SubGlass {
            rect: knob,
            style: GlassStyle::knob(),
            engaged: info.press > 0.5,
        });
    }
}

pub(super) fn paint_drag(drag: &DragValueContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let look = info.theme.surface(Role::Field, crate::style::State::Rest);
    painter.outlined(info.rect, look.corners, look.fill, look.border, look.shadow);
    let text = format!("{:.*}", drag.decimals, drag.value);
    painter.text_in(
        info.rect.shrink2(Vec2::new(9.0, 0.0)),
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
    painter.rect(rect, Corners::capsule(), palette.track);
    let filled = rect.width() * progress.fraction.clamp(0.0, 1.0);
    if filled > 1.0 {
        painter.gradient(
            Rect::from_min_size(rect.min, Vec2::new(filled, rect.height())),
            Corners::capsule(),
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
}
