use glam::Vec2;

use crate::content::{CheckboxContent, Content, ToggleContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::motion::Motion;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Metrics, Theme};

use super::{PaintInfo, SubGlass};

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
        size: Size::new(Length::Auto, Length::Fixed(26.0)),
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
    Vec2::new(measured + 28.0, 26.0)
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
        let knob_height = (rect.height() - 2.0).max(1.0);
        let knob_width = knob_height * 1.25;
        let track_height = knob_height * 0.76;
        let overhang = (knob_height - track_height) * 0.5;
        let track = Rect::from_center_size(
            rect.center(),
            Vec2::new((rect.width() - overhang * 2.0).max(1.0), track_height),
        );
        Self {
            track,
            knob: Vec2::new(knob_width, knob_height),
            travel: (rect.width() - knob_width).max(0.0),
        }
    }

    fn center(&self, rect: Rect, progress: f32) -> f32 {
        rect.min.x + self.knob.x * 0.5 + self.travel * progress
    }
}

pub(crate) fn drag_value(rect: Rect, pointer: Vec2) -> f32 {
    let switch = Switch::new(rect);
    ((pointer.x - rect.min.x - switch.knob.x * 0.5) / switch.travel.max(1.0)).clamp(0.0, 1.0)
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
        Motion::response(0.42, 0.30)
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
    Some((Motion::response(0.28, 0.28), (target, 0.0)))
}

pub(super) fn paint_toggle(
    toggle: &ToggleContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
) {
    let rect = info.rect;
    let switch = switch_rect(toggle, rect, &info.theme.metrics);
    if toggle.row {
        if info.hover > 0.01 {
            painter.rect(
                rect.shrink2(Vec2::new(1.0, 0.5)),
                Corners::all(9.0),
                info.theme.palette.material_hover.amplify(0.55 * info.hover),
            );
        }
        if let Some(label) = &toggle.label {
            painter.text_in(
                Rect::from_min_max(rect.min, Vec2::new(switch.min.x - 10.0, rect.max.y)),
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
    paint_switch(runtime, painter, info, switch, glass);
}

fn paint_switch(
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    rect: Rect,
    glass: &mut Vec<SubGlass>,
) {
    let palette = info.theme.palette;
    let progress = runtime.value.value.clamp(-0.06, 1.06);
    let switch = Switch::new(rect);
    let center = switch.center(rect, progress);
    let knob_rect = Rect::from_center_size(Vec2::new(center, rect.center().y), switch.knob);
    let fill = palette
        .track
        .lerp(palette.success, progress.clamp(0.0, 1.0));
    painter.rect(switch.track, Corners::capsule(), fill);
    glass.push(SubGlass {
        rect: knob_rect,
        style: GlassStyle::knob(),
        engaged: info.press > 0.5,
    });
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
        Vec2::new(rect.min.x + 9.0, rect.center().y),
        Vec2::splat(17.0),
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
        Corners::all(5.0),
        Fill::Solid(fill),
        Border {
            width: 1.2,
            color: outline.lerp(palette.accent, mark),
        },
        Shadow::NONE,
    );
    if mark > 0.02 {
        let color = palette.text_on_accent.with_alpha(mark);
        let left = Vec2::new(box_rect.min.x + 4.2, box_rect.min.y + 8.6);
        let bottom = Vec2::new(box_rect.min.x + 7.2, box_rect.max.y - 4.4);
        let right = Vec2::new(box_rect.max.x - 3.6, box_rect.min.y + 4.8);
        painter.segment(left, bottom, 1.9, color);
        painter.segment(bottom, right, 1.9, color);
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 24.0, rect.min.y), rect.max),
        &checkbox.label,
        TextStyle::new(checkbox.font, text_color),
        Align::Center,
    );
}
