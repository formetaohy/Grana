use glam::Vec2;

use crate::color::Color;
use crate::content::{CheckboxContent, Content, ToggleContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Metrics, Theme};

use super::PaintInfo;

pub fn toggle<'a, A>(theme: &Theme, value: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(
            Length::Fixed(theme.metrics.switch_width),
            Length::Fixed(theme.metrics.switch_height),
        ),
        ..Layout::row()
    })
    .with_content(Content::Toggle(ToggleContent {
        value,
        label: None,
        font: theme.font(crate::theme::Level::Callout).medium(),
        row: false,
    }))
}

pub fn toggle_row<'a, A>(theme: &Theme, label: &str, value: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(theme.metrics.row_height)),
        ..Layout::row()
    })
    .with_content(Content::Toggle(ToggleContent {
        value,
        label: Some(label.to_string()),
        font: theme.font(crate::theme::Level::Callout).medium(),
        row: true,
    }))
}

pub fn checkbox<'a, A>(theme: &Theme, label: &str, value: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Auto, Length::Fixed(24.0)),
        ..Layout::row()
    })
    .with_content(Content::Checkbox(CheckboxContent {
        value,
        label: label.to_string(),
        font: theme.font(crate::theme::Level::Callout),
    }))
}

pub(super) fn measure_toggle(toggle: &ToggleContent, available: Vec2, theme: &Theme) -> Vec2 {
    if toggle.row {
        Vec2::new(available.x, theme.metrics.row_height)
    } else {
        Vec2::new(theme.metrics.switch_width, theme.metrics.switch_height)
    }
}

pub(super) fn measure_checkbox(checkbox: &CheckboxContent, text: &mut crate::text::Text) -> Vec2 {
    let measured = text
        .layout(
            &checkbox.label,
            checkbox.font,
            0.0,
            false,
            false,
            Align::Start,
        )
        .size
        .x;
    Vec2::new(measured + 24.0, 24.0)
}

pub(crate) fn switch_rect(toggle: &ToggleContent, rect: Rect, metrics: &Metrics) -> Rect {
    if toggle.row {
        Rect::from_center_size(
            Vec2::new(rect.max.x - metrics.switch_width * 0.5, rect.center().y),
            Vec2::new(metrics.switch_width, metrics.switch_height),
        )
    } else {
        rect
    }
}

struct Switch {
    track: Rect,
    knob: Vec2,
    travel: f32,
}

impl Switch {
    fn new(rect: Rect) -> Self {
        let knob_size = (rect.height() - 4.0).max(10.0);
        let inset = 2.0;
        let travel = (rect.width() - knob_size - inset * 2.0).max(0.0);
        Self {
            track: rect,
            knob: Vec2::splat(knob_size),
            travel,
        }
    }

    fn center(&self, rect: Rect, progress: f32) -> f32 {
        rect.min.x + 2.0 + self.knob.x * 0.5 + self.travel * progress
    }
}

pub(crate) fn drag_value(rect: Rect, pointer: Vec2) -> f32 {
    let switch = Switch::new(rect);
    ((pointer.x - rect.min.x - 2.0 - switch.knob.x * 0.5) / switch.travel.max(1.0)).clamp(0.0, 1.0)
}

pub(crate) fn normalized(toggle: &ToggleContent) -> f32 {
    if toggle.value { 1.0 } else { 0.0 }
}

pub(crate) fn value_toggle<A>(node: &Node<A>, dragging: bool) -> Option<(Motion, (f32, f32))> {
    let Content::Toggle(toggle) = &node.content else {
        return None;
    };
    let motion = if dragging {
        Motion::INSTANT
    } else {
        Motion::snappy()
    };
    let target = if dragging {
        node.runtime.drag_value
    } else {
        node.runtime.commit.unwrap_or_else(|| normalized(toggle))
    };
    Some((motion, (target, 0.0)))
}

pub(crate) fn value_checkbox<A>(node: &Node<A>) -> Option<(Motion, (f32, f32))> {
    let Content::Checkbox(checkbox) = &node.content else {
        return None;
    };
    let target = if checkbox.value { 1.0 } else { 0.0 };
    Some((Motion::snappy(), (target, 0.0)))
}

pub(super) fn paint_toggle(
    toggle: &ToggleContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let rect = info.rect;
    let switch = switch_rect(toggle, rect, &info.theme.metrics);
    if toggle.row {
        if info.hover > 0.01 {
            painter.rect(
                rect.shrink2(Vec2::new(1.0, 0.5)),
                Corners::all(info.theme.metrics.radius),
                info.theme.palette.material_hover.amplify(0.5 * info.hover),
            );
        }
        if let Some(label) = &toggle.label {
            painter.text_in(
                Rect::from_min_max(rect.min, Vec2::new(switch.min.x - 8.0, rect.max.y)),
                label,
                TextStyle::new(
                    toggle.font,
                    info.theme.palette.text_secondary.lerp(
                        info.theme.palette.text_primary,
                        info.hover.max(if toggle.value { 1.0 } else { 0.0 }),
                    ),
                )
                .truncating(),
                Align::Center,
            );
        }
    }
    paint_switch(runtime, painter, info, switch);
}

fn paint_switch(runtime: &Runtime, painter: &mut Painter, info: &PaintInfo<'_>, rect: Rect) {
    let palette = info.theme.palette;
    let progress = runtime.value.value.clamp(0.0, 1.0);
    let switch = Switch::new(rect);
    let center = switch.center(rect, progress);
    let knob_rect = Rect::from_center_size(Vec2::new(center, rect.center().y), switch.knob);
    let fill = palette.track.lerp(palette.accent, progress);
    painter.outlined(
        switch.track,
        Corners::capsule(),
        Fill::Solid(fill),
        Border {
            width: 1.0,
            color: palette
                .material_border
                .lerp(palette.accent_hover.with_alpha(0.55), progress),
        },
        Shadow::NONE,
    );
    painter.outlined(
        knob_rect,
        Corners::capsule(),
        Fill::Gradient(Color::bytes(250, 251, 253), Color::bytes(222, 226, 232)),
        Border::NONE,
        Shadow::new(Color::black_alpha(0.38), 3.0, 1.0),
    );
}

pub(super) fn paint_checkbox(
    checkbox: &CheckboxContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let rect = info.rect;
    let mark = runtime.value.value;
    let box_rect = Rect::from_center_size(
        Vec2::new(rect.min.x + 8.0, rect.center().y),
        Vec2::splat(15.0),
    );
    let outline = palette
        .text_tertiary
        .lerp(palette.text_secondary, info.hover);
    let fill = palette.accent.with_alpha(mark);
    let text_color = palette
        .text_secondary
        .lerp(palette.text_primary, info.hover.max(mark));
    painter.outlined(
        box_rect,
        Corners::all(4.0),
        Fill::Solid(fill),
        Border {
            width: 1.2,
            color: outline.lerp(palette.accent, mark),
        },
        Shadow::NONE,
    );
    if mark > 0.02 {
        let color = palette.text_on_accent.with_alpha(mark);
        let left = Vec2::new(box_rect.min.x + 3.6, box_rect.min.y + 7.6);
        let bottom = Vec2::new(box_rect.min.x + 6.4, box_rect.max.y - 3.8);
        let right = Vec2::new(box_rect.max.x - 3.2, box_rect.min.y + 4.2);
        painter.segment(left, bottom, 1.8, color);
        painter.segment(bottom, right, 1.8, color);
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 21.0, rect.min.y), rect.max),
        &checkbox.label,
        TextStyle::new(checkbox.font, text_color),
        Align::Center,
    );
}
