use glam::Vec2;

use crate::content::{CheckboxContent, Content, ToggleContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::icon::{self, Icon};
use crate::motion::Motion;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Metrics, Theme};

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
        font: theme.font(Level::Body).medium(),
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
        font: theme.font(Level::Body).medium(),
        row: true,
    }))
}

pub fn checkbox<'a, A>(theme: &Theme, label: &str, value: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Auto, Length::Fixed(26.0)),
        ..Layout::row()
    })
    .with_content(Content::Checkbox(CheckboxContent {
        value,
        label: label.to_string(),
        font: theme.font(Level::Label),
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
    Vec2::new(measured + 26.0, 26.0)
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

pub(crate) fn drag_value(rect: Rect, pointer: Vec2) -> f32 {
    let min_x = rect.min.x + 11.0;
    let max_x = rect.max.x - 11.0;
    ((pointer.x - min_x) / (max_x - min_x).max(1.0)).clamp(0.0, 1.0)
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
        Motion::spatial_fast()
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
    Some((Motion::spatial_fast(), (target, 0.0)))
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
                Corners::all(info.theme.metrics.corner_sm),
                info.theme.palette.on_surface.with_alpha(0.07 * info.hover),
            );
        }
        if let Some(label) = &toggle.label {
            painter.text_in(
                Rect::from_min_max(rect.min, Vec2::new(switch.min.x - 8.0, rect.max.y)),
                label,
                TextStyle::new(
                    toggle.font,
                    info.theme.palette.on_surface_variant.lerp(
                        info.theme.palette.on_surface,
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
    let fill = palette
        .surface_container_highest
        .lerp(palette.primary, progress);
    let border_color = palette.outline.with_alpha((1.0 - progress) * 0.9);
    painter.outlined(
        rect,
        Corners::capsule(),
        Fill::Solid(fill),
        Border {
            width: 1.5 * (1.0 - progress),
            color: border_color,
        },
        Shadow::NONE,
    );
    let diameter = 11.0 + 5.5 * progress + 2.0 * info.press;
    let start_x = rect.min.x + 10.5;
    let end_x = rect.max.x - 10.5;
    let center_x = start_x + (end_x - start_x) * progress;
    let center = Vec2::new(center_x, rect.center().y);
    let thumb_rect = Rect::from_center_size(center, Vec2::splat(diameter));
    let thumb_color = palette.outline.lerp(palette.on_primary, progress);
    painter.rect(thumb_rect, Corners::capsule(), thumb_color);
    if progress > 0.35 {
        let alpha = ((progress - 0.35) / 0.65).clamp(0.0, 1.0);
        icon::paint(
            Icon::Check,
            painter,
            center,
            10.5,
            palette.primary.with_alpha(alpha),
            1.8,
        );
    }
}

pub(super) fn paint_checkbox(
    checkbox: &CheckboxContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let rect = info.rect;
    let mark = runtime.value.value.clamp(0.0, 1.0);
    let box_rect = Rect::from_center_size(
        Vec2::new(rect.min.x + 9.0, rect.center().y),
        Vec2::splat(16.0),
    );
    let outline = palette.outline.lerp(palette.on_surface, info.hover);
    let fill = palette.primary.with_alpha(mark);
    let text_color = palette
        .on_surface_variant
        .lerp(palette.on_surface, info.hover.max(mark));
    painter.outlined(
        box_rect,
        Corners::all(5.0),
        Fill::Solid(fill),
        Border {
            width: 1.5 * (1.0 - mark),
            color: outline,
        },
        Shadow::NONE,
    );
    if mark > 0.05 {
        icon::paint(
            Icon::Check,
            painter,
            box_rect.center(),
            11.5,
            palette.on_primary.with_alpha(mark),
            1.9,
        );
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 23.0, rect.min.y), rect.max),
        &checkbox.label,
        TextStyle::new(checkbox.font, text_color),
        Align::Center,
    );
}
