use glam::Vec2;

use crate::content::Content;
use crate::element::Layout;
use crate::geometry::{Corners, Rect};
use crate::input::Cursor;
use crate::motion::{Animated, Motion};
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::text::Text;
use crate::theme::Theme;

pub(crate) mod button;
pub(crate) mod color;
pub(crate) mod field;
pub(crate) mod label;
pub(crate) mod menu;
pub(crate) mod segment;
pub(crate) mod slider;
pub(crate) mod toggle;

pub use button::{
    Split, action_button, button, button_enabled, button_width, icon_button, split_button,
};
pub use color::color_picker;
pub use field::text_field;
pub use label::{badge, caption, color_swatch, labelled, spinner, weak};
pub use menu::{action_row, menu_row, separator};
pub use segment::{segmented, segmented_fill, tabs};
pub use slider::{drag_value, gradient_bar, progress, range_slider, slider_readout, slider_sized};
pub use toggle::{checkbox, toggle, toggle_row};

pub struct PaintInfo<'a> {
    pub theme: &'a Theme,
    pub rect: Rect,
    pub hover: f32,
    pub press: f32,
    pub engaged: f32,
    pub time: f32,
    pub enabled: bool,
    pub preedit: Option<&'a str>,
}

pub(crate) struct FrameAnimation {
    pub hovered: bool,
    pub pressed: bool,
    pub dragging: bool,
    pub pointer: Vec2,
    pub elapsed: f32,
}

pub(crate) fn measure(
    content: &Content,
    layout: Layout,
    available: Vec2,
    text: &mut Text,
    theme: &Theme,
) -> (Vec2, Option<f32>) {
    match content {
        Content::Button(button) => (button::measure(button, available, text), None),
        Content::IconButton(icon) => (Vec2::splat(icon.size), None),
        Content::Toggle(toggle) => (toggle::measure_toggle(toggle, available, theme), None),
        Content::Checkbox(checkbox) => (toggle::measure_checkbox(checkbox, text), None),
        Content::Slider(slider) => (slider::measure_slider(slider, available, theme), None),
        Content::RangeSlider(range) => (slider::measure_range(range, available), None),
        Content::Segment(segment) => (
            segment::measure_segment(segment, available, text, theme),
            None,
        ),
        Content::Tabs(tabs) => (segment::measure_tabs(tabs, available), None),
        Content::DragValue(drag) => (slider::measure_drag(drag, theme), None),
        Content::Field(field) => (field::measure_field(field, layout, available), None),
        Content::MenuRow(_) => (Vec2::new(available.x, menu::MENU_ROW), None),
        Content::ActionRow(_) => (Vec2::new(available.x, menu::MENU_ROW), None),
        Content::Badge(badge) => (label::measure_badge(badge, text), None),
        Content::Spinner(spinner) => (Vec2::splat(spinner.size), None),
        Content::Progress(progress) => (Vec2::new(progress.width, 10.0), None),
        Content::GradientBar(bar) => (Vec2::new(available.x, bar.height), None),
        Content::Labelled(_) => (Vec2::new(available.x, 22.0), None),
        Content::ColorSwatch(swatch) => (Vec2::splat(swatch.size), None),
        Content::ColorPicker(_) => (color::measure(), None),
        Content::Separator => (Vec2::new(available.x, menu::SEPARATOR), None),
        Content::Empty | Content::Canvas | Content::Text(_) => (
            Vec2::new(
                layout.size.width.resolve(available.x),
                layout.size.height.resolve(available.y),
            ),
            None,
        ),
    }
}

pub(crate) fn paint(
    content: &Content,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) -> Option<Rect> {
    match content {
        Content::Empty | Content::Canvas => None,
        Content::Text(text) => {
            let max = if text.style.wrap {
                info.rect.width()
            } else {
                0.0
            };
            let layout = painter.layout(&text.text, text.style, max);
            painter.text(&layout, info.rect.min, text.style.color);
            None
        }
        Content::Button(button) => {
            button::paint(button, painter, info);
            None
        }
        Content::IconButton(icon) => {
            button::paint_icon(icon, painter, info);
            None
        }
        Content::Toggle(toggle) => {
            toggle::paint_toggle(toggle, runtime, painter, info);
            None
        }
        Content::Checkbox(checkbox) => {
            toggle::paint_checkbox(checkbox, runtime, painter, info);
            None
        }
        Content::Slider(slider) => {
            slider::paint_slider(slider, runtime, painter, info);
            None
        }
        Content::RangeSlider(range) => {
            slider::paint_range(range, runtime, painter, info);
            None
        }
        Content::Segment(segment) => {
            segment::paint_segment(segment, runtime, painter, info);
            None
        }
        Content::Tabs(tabs) => {
            segment::paint_tabs(tabs, runtime, painter, info);
            None
        }
        Content::DragValue(drag) => {
            slider::paint_drag(drag, painter, info);
            None
        }
        Content::Field(field) => field::paint_field(field, runtime, painter, info),
        Content::MenuRow(row) => {
            menu::paint_menu_row(row, runtime, painter, info);
            None
        }
        Content::ActionRow(row) => {
            menu::paint_action_row(row, runtime, painter, info);
            None
        }
        Content::Badge(badge) => {
            label::paint_badge(badge, painter, info);
            None
        }
        Content::Spinner(spinner) => {
            label::paint_spinner(spinner, painter, info);
            None
        }
        Content::Progress(progress) => {
            slider::paint_progress(progress, painter, info);
            None
        }
        Content::GradientBar(bar) => {
            slider::paint_gradient_bar(bar, painter, info);
            None
        }
        Content::Labelled(labelled) => {
            label::paint_labelled(labelled, painter, info);
            None
        }
        Content::ColorSwatch(swatch) => {
            label::paint_color_swatch(swatch, painter, info);
            None
        }
        Content::ColorPicker(color) => {
            color::paint(color, runtime, painter, info);
            None
        }
        Content::Separator => {
            menu::paint_separator(painter, info);
            None
        }
    }
}

pub(crate) fn animate<A>(
    node: &mut Node<A>,
    frame: &FrameAnimation,
    text: &mut Text,
    theme: &Theme,
) -> bool {
    let interactive = node.interaction.is_interactive();
    let hovered = frame.hovered && interactive;
    let active = frame.pressed && interactive;
    let press = if active {
        Motion::effect_fast()
    } else if theme.reduce_motion {
        Motion::effect_default()
    } else {
        Motion::spatial_fast()
    };
    node.runtime.hover.animate_to(
        if hovered { 1.0 } else { 0.0 },
        Motion::effect_fast().spring(),
    );
    node.runtime
        .press
        .animate_to(if active { 1.0 } else { 0.0 }, press.spring());
    if let Some((motion, target)) = value_target(node, frame, text, theme) {
        let motion = if theme.reduce_motion {
            motion.reduced()
        } else {
            motion
        };
        node.runtime.engaged.animate_to(
            if hovered || active { 1.0 } else { 0.0 },
            Motion::effect_fast().spring(),
        );
        if node.runtime.seeded {
            if motion.is_instant() {
                let changed =
                    node.runtime.value.value != target.0 || node.runtime.extent.value != target.1;
                node.runtime.value.snap(target.0);
                node.runtime.extent.snap(target.1);
                node.paint_dirty |= changed;
            } else {
                node.runtime.value.animate_to(target.0, motion.spring());
                node.runtime.extent.animate_to(target.1, motion.spring());
            }
        } else {
            node.runtime.value = Animated::new(target.0, motion.spring());
            node.runtime.extent = Animated::new(target.1, motion.spring());
            node.runtime.seeded = true;
            node.paint_dirty = true;
        }
    }
    node.runtime.commit = None;
    let mut animating = false;
    for animation in [
        &mut node.runtime.hover,
        &mut node.runtime.press,
        &mut node.runtime.value,
        &mut node.runtime.extent,
        &mut node.runtime.engaged,
    ] {
        if !animation.is_at_rest() {
            animation.step(frame.elapsed);
            animating = true;
        }
    }
    if matches!(node.content, Content::Spinner(_) | Content::Progress(_)) && !theme.reduce_motion {
        node.paint_dirty = true;
        animating = true;
    }
    if animating {
        node.paint_dirty = true;
    }
    animating
}

fn value_target<A>(
    node: &Node<A>,
    frame: &FrameAnimation,
    text: &mut Text,
    theme: &Theme,
) -> Option<(Motion, (f32, f32))> {
    match &node.content {
        Content::Toggle(_) => toggle::value_toggle(node, frame.dragging),
        Content::Checkbox(_) => toggle::value_checkbox(node),
        Content::Slider(_) => slider::value_slider(node, frame.pressed),
        Content::RangeSlider(_) => slider::value_range(node, frame.pressed),
        Content::Segment(_) => {
            segment::value_segment(node, frame.dragging, frame.pointer, text, theme)
        }
        Content::Tabs(_) => segment::value_tabs(node, frame.dragging, frame.pointer, text, theme),
        Content::ColorPicker(_) => color::value(node),
        _ => None,
    }
}

pub(crate) fn hit_rect<A>(node: &Node<A>, text: &mut Text, theme: &Theme) -> Rect {
    match &node.content {
        Content::Slider(slider) => slider::track(slider, node.rect, text, theme),
        _ => node.rect,
    }
}

pub(crate) fn focus_corners(content: &Content, theme: &Theme) -> Corners {
    match content {
        Content::Toggle(_)
        | Content::Button(_)
        | Content::IconButton(_)
        | Content::Segment(_)
        | Content::Tabs(_)
        | Content::ColorSwatch(_) => Corners::capsule(),
        Content::Checkbox(_) => Corners::all(5.0),
        Content::Slider(_)
        | Content::RangeSlider(_)
        | Content::DragValue(_)
        | Content::Field(_)
        | Content::MenuRow(_)
        | Content::ActionRow(_)
        | Content::ColorPicker(_) => Corners::all(theme.metrics.corner_sm),
        _ => Corners::ZERO,
    }
}

pub(crate) fn cursor(content: &Content) -> Option<Cursor> {
    match content {
        Content::Button(_)
        | Content::IconButton(_)
        | Content::Toggle(_)
        | Content::Checkbox(_)
        | Content::Segment(_)
        | Content::Tabs(_)
        | Content::MenuRow(_)
        | Content::ActionRow(_)
        | Content::ColorSwatch(_) => Some(Cursor::Pointing),
        Content::Slider(_) | Content::RangeSlider(_) | Content::DragValue(_) => {
            Some(Cursor::ResizeHorizontal)
        }
        Content::Field(_) => Some(Cursor::Text),
        Content::ColorPicker(_) => Some(Cursor::Crosshair),
        _ => None,
    }
}

pub(crate) fn padding(theme: &Theme) -> f32 {
    (theme.metrics.control_height * 0.44).round()
}
