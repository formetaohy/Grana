use glam::Vec2;

use crate::element::{Content, Layout};
use crate::geometry::Rect;
use crate::glass::GlassStyle;
use crate::node::{Node, Runtime};
use crate::paint::Painter;
use crate::text::Text;
use crate::theme::Theme;

mod button;
pub(crate) mod color;
mod disclosure;
mod field;
mod label;
mod menu;
mod segment;
mod slider;
mod toggle;

pub use button::{
    action_button, button, button_enabled, button_width, glass_action_button, glass_button,
    glass_icon_button, glass_icon_button_disabled, icon_button, icon_button_disabled,
};
pub use color::color_picker;
pub use disclosure::disclosure;
pub use field::{text_area, text_field};
pub use label::{
    badge, body, body_wrapped, caption, color_swatch, headline, labelled, section_title, spinner,
    title, weak,
};
pub use menu::{MENU_ROW, action_row, menu_row, separator};
pub(crate) use segment::hover_cell;
pub use segment::{segmented, segmented_fill, tab_bar, tabs};
pub use slider::{
    drag_value, gradient_bar, progress, range_slider, slider, slider_readout, slider_sized,
};
pub use toggle::{checkbox, toggle, toggle_row};

#[derive(Clone, Copy)]
pub(crate) struct SubGlass {
    pub rect: Rect,
    pub style: GlassStyle,
    pub engaged: bool,
}

pub struct PaintInfo<'a> {
    pub theme: &'a Theme,
    pub rect: Rect,
    pub glass: Option<Rect>,
    pub hover: f32,
    pub press: f32,
    pub engaged: f32,
    pub time: f32,
    pub enabled: bool,
    pub focused: bool,
}

pub(crate) fn measure(
    content: &Content,
    layout: Layout,
    available: Vec2,
    text: &mut Text,
    theme: &Theme,
) -> (Vec2, Option<f32>) {
    let size = match content {
        Content::Button(button) => button::measure_button(button, available, text, theme),
        Content::IconButton(icon) => Vec2::splat(icon.size),
        Content::Toggle(toggle) => toggle::measure_toggle(toggle, available, theme),
        Content::Checkbox(checkbox) => toggle::measure_checkbox(checkbox, available, text),
        Content::Slider(slider) => slider::measure_slider(slider, available, text, theme),
        Content::RangeSlider(range) => slider::measure_range(range, available),
        Content::Segment(segment) => segment::measure_segment(segment, available, text, theme),
        Content::Tabs(tabs) => segment::measure_tabs(tabs, available, text, theme),
        Content::DragValue(drag) => slider::measure_drag(drag, theme),
        Content::Field(field) => field::measure_field(field, layout, available),
        Content::Disclosure(_) => Vec2::new(available.x, 26.0),
        Content::MenuRow(_) => Vec2::new(available.x, menu::MENU_ROW),
        Content::ActionRow(row) => menu::measure_action(row, available, text),
        Content::Badge(badge) => label::measure_badge(badge, text),
        Content::Spinner(spinner) => Vec2::splat(spinner.size),
        Content::Progress(progress) => Vec2::new(progress.width, 4.0),
        Content::GradientBar(bar) => Vec2::new(available.x, bar.height),
        Content::Labelled(_) => Vec2::new(available.x, 22.0),
        Content::SectionTitle(_) => Vec2::new(available.x, 24.0),
        Content::ColorSwatch(swatch) => Vec2::splat(swatch.size),
        Content::ColorPicker(_) => color::measure(),
        Content::Separator => Vec2::new(available.x, 9.0),
        Content::Empty | Content::Canvas | Content::Text(_) => Vec2::new(
            layout.size.width.resolve(available.x),
            layout.size.height.resolve(available.y),
        ),
    };
    (size, None)
}

pub(crate) fn paint(
    content: &Content,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
    glass: &mut Vec<SubGlass>,
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
            button::paint_button(button, runtime, painter, info);
            None
        }
        Content::IconButton(icon) => {
            button::paint_icon_button(icon, runtime, painter, info);
            None
        }
        Content::Toggle(toggle) => {
            toggle::paint_toggle(toggle, runtime, painter, info, glass);
            None
        }
        Content::Checkbox(checkbox) => {
            toggle::paint_checkbox(checkbox, runtime, painter, info);
            None
        }
        Content::Slider(slider) => {
            slider::paint_slider(slider, runtime, painter, info, glass);
            None
        }
        Content::RangeSlider(range) => {
            slider::paint_range(range, runtime, painter, info, glass);
            None
        }
        Content::Segment(segment) => {
            segment::paint_segment(segment, runtime, painter, info, glass);
            None
        }
        Content::Tabs(tabs) => {
            segment::paint_tabs(tabs, runtime, painter, info, glass);
            None
        }
        Content::DragValue(drag) => {
            slider::paint_drag(drag, painter, info);
            None
        }
        Content::Field(field) => field::paint_field(field, runtime, painter, info),
        Content::Disclosure(disclosure) => {
            disclosure::paint_disclosure(disclosure, runtime, painter, info);
            None
        }
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
        Content::SectionTitle(title) => {
            label::paint_section_title(title, painter, info);
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn animate<A>(
    node: &mut Node<A>,
    hovered: bool,
    active: bool,
    elapsed: f32,
    physics: bool,
    reduce_motion: bool,
    text: &mut Text,
    theme: &Theme,
) -> bool {
    let adjust = |motion: crate::motion::Motion| {
        if !physics {
            crate::motion::Motion::INSTANT
        } else if reduce_motion {
            motion.reduced()
        } else {
            motion
        }
    };
    let interactive = node.interaction.is_interactive();
    let hovered = hovered && interactive;
    let active = active && interactive;
    let press_motion = if active {
        crate::motion::Motion::response(0.22, 0.0)
    } else {
        crate::motion::Motion::bouncy()
    };
    node.runtime.hover.animate_to(
        if hovered { 1.0 } else { 0.0 },
        adjust(crate::motion::Motion::quick()).spring(),
    );
    node.runtime.press.animate_to(
        if active { 1.0 } else { 0.0 },
        adjust(press_motion).spring(),
    );
    if let Some(text_motion) = node_value_motion(node, active) {
        node.runtime.engaged.animate_to(
            if hovered || active { 1.0 } else { 0.0 },
            adjust(crate::motion::Motion::quick()).spring(),
        );
        if let Some(target) = node_value_target(node, text, theme) {
            if !node.runtime.seeded {
                node.runtime.value =
                    grana_glass::Animated::new(target.0, adjust(text_motion).spring());
                node.runtime.extent =
                    grana_glass::Animated::new(target.1, adjust(text_motion).spring());
                node.runtime.seeded = true;
                node.paint_dirty = true;
            } else {
                node.runtime
                    .value
                    .animate_to(target.0, adjust(text_motion).spring());
                node.runtime
                    .extent
                    .animate_to(target.1, adjust(text_motion).spring());
            }
        }
    }
    let mut animating = false;
    for animation in [
        &mut node.runtime.hover,
        &mut node.runtime.press,
        &mut node.runtime.value,
        &mut node.runtime.extent,
        &mut node.runtime.engaged,
    ] {
        if !animation.is_at_rest() {
            animation.step(elapsed);
            animating = true;
        }
    }
    if matches!(node.content, Content::Spinner(_)) {
        node.paint_dirty = true;
        animating = true;
    }
    if animating {
        node.paint_dirty = true;
    }
    animating
}

fn node_value_motion<A>(node: &Node<A>, active: bool) -> Option<crate::motion::Motion> {
    match &node.content {
        Content::Toggle(_) if node.runtime.dragging && active => {
            Some(crate::motion::Motion::response(0.14, 0.0))
        }
        Content::Toggle(_) => Some(crate::motion::Motion::response(0.42, 0.30)),
        Content::Checkbox(_) => Some(crate::motion::Motion::response(0.28, 0.28)),
        Content::Slider(_) => Some(if active {
            crate::motion::Motion::response(0.18, 0.18)
        } else {
            crate::motion::Motion::response(0.32, 0.2)
        }),
        Content::RangeSlider(_) => Some(if active {
            crate::motion::Motion::response(0.16, 0.12)
        } else {
            crate::motion::Motion::response(0.32, 0.2)
        }),
        Content::Segment(_) => Some(if node.runtime.dragging && active {
            crate::motion::Motion::response(0.16, 0.12)
        } else {
            crate::motion::Motion::response(0.42, 0.22)
        }),
        Content::Tabs(_) => Some(if node.runtime.dragging && active {
            crate::motion::Motion::response(0.16, 0.12)
        } else {
            crate::motion::Motion::response(0.42, 0.20)
        }),
        Content::Disclosure(disclosure) => {
            let _ = disclosure;
            Some(crate::motion::Motion::response(0.32, 0.18))
        }
        _ => None,
    }
}

fn node_value_target<A>(node: &Node<A>, text: &mut Text, theme: &Theme) -> Option<(f32, f32)> {
    match &node.content {
        Content::Toggle(toggle) => {
            let target = if node.runtime.dragging && node.runtime.press.target > 0.0 {
                node.runtime.value.target
            } else if toggle.value {
                1.0
            } else {
                0.0
            };
            Some((target, 0.0))
        }
        Content::Slider(slider) => {
            let (minimum, maximum) = (*slider.range.start(), *slider.range.end());
            let value = slider.value.clamp(minimum, maximum);
            let target = if slider.logarithmic && minimum > 0.0 {
                (value.max(minimum).ln() - minimum.ln()) / (maximum.ln() - minimum.ln())
            } else {
                (value - minimum) / (maximum - minimum)
            };
            Some((target.clamp(0.0, 1.0), 0.0))
        }
        Content::RangeSlider(range) => {
            let (minimum, maximum) = (*range.range.start(), *range.range.end());
            let span = (maximum - minimum).max(1.0e-6);
            Some((
                ((range.start - minimum) / span).clamp(0.0, 1.0),
                ((range.end - minimum) / span).clamp(0.0, 1.0),
            ))
        }
        Content::Segment(segment) => {
            if node.runtime.dragging && node.runtime.press.target > 0.0 {
                return Some((node.runtime.value.target, node.runtime.extent.target));
            }
            let (x, width) = segment::pill_target(segment, node.rect, text, theme);
            Some((x, width))
        }
        Content::Tabs(tabs) => {
            if node.runtime.dragging && node.runtime.press.target > 0.0 {
                return Some((node.runtime.value.target, node.runtime.extent.target));
            }
            let (x, width) = segment::tabs_pill_target(tabs, node.rect, text, theme);
            Some((x, width))
        }
        Content::Checkbox(checkbox) => Some((if checkbox.value { 1.0 } else { 0.0 }, 0.0)),
        Content::Disclosure(_) => Some((if node.runtime.open { 1.0 } else { 0.0 }, 0.0)),
        Content::ColorPicker(color) => {
            let hue = if node.runtime.seeded {
                node.runtime.value.value
            } else {
                color::rgb_to_hsv(color.color).0
            };
            Some((hue, 0.0))
        }
        _ => None,
    }
}

pub(crate) fn hit_rect<A>(node: &Node<A>, text: &mut Text, theme: &Theme) -> Option<Rect> {
    match &node.content {
        Content::Disclosure(_) => Some(Rect::from_min_size(
            node.rect.min,
            Vec2::new(node.rect.width(), 26.0),
        )),
        Content::Slider(slider) => Some(slider::track_rect(slider, node.rect, text, theme)),
        _ => Some(node.rect),
    }
}

pub(crate) fn padding(theme: &Theme) -> f32 {
    (theme.metrics.control_height * 0.5).round() - 3.0
}

pub(crate) fn press_grow(press: f32) -> f32 {
    1.4 * press
}

pub(crate) fn slider_track(content: &Content, rect: Rect, text: &mut Text, theme: &Theme) -> Rect {
    match content {
        Content::Slider(slider) => slider::track_rect(slider, rect, text, theme),
        _ => rect,
    }
}
